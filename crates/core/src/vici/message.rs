//! Mensagens do protocolo VICI: modelo, codificação e decodificação.
//!
//! Formato de uma mensagem (sequência de elementos):
//!
//! | Elemento        | Código | Conteúdo                              |
//! |-----------------|--------|---------------------------------------|
//! | `SECTION_START` | 1      | nome (u8 + bytes)                     |
//! | `SECTION_END`   | 2      | —                                     |
//! | `KEY_VALUE`     | 3      | chave (u8 + bytes), valor (u16 + bytes)|
//! | `LIST_START`    | 4      | nome (u8 + bytes)                     |
//! | `LIST_ITEM`     | 5      | valor (u16 + bytes)                   |
//! | `LIST_END`      | 6      | —                                     |
//!
//! Segurança: todo valor fica num buffer zerado no drop ([`Zeroizing`]), e o
//! `Debug` nunca mostra valores, porque mensagens podem carregar PSK e senha
//! XAuth (`load-shared`).

use std::fmt;

use zeroize::Zeroizing;

use super::ViciError;

const SECTION_START: u8 = 1;
const SECTION_END: u8 = 2;
const KEY_VALUE: u8 = 3;
const LIST_START: u8 = 4;
const LIST_ITEM: u8 = 5;
const LIST_END: u8 = 6;

/// Profundidade máxima de seções aninhadas aceita na decodificação.
const MAX_DEPTH: usize = 32;

/// Um valor de mensagem VICI.
#[derive(Clone, PartialEq, Eq)]
pub enum Value {
    /// Par chave/valor (bytes, normalmente UTF-8).
    Str(Zeroizing<Vec<u8>>),
    /// Seção aninhada.
    Section(Message),
    /// Lista de valores.
    List(Vec<Zeroizing<Vec<u8>>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(_) => f.write_str("<valor>"),
            Self::Section(m) => m.fmt(f),
            Self::List(items) => write!(f, "<lista com {} itens>", items.len()),
        }
    }
}

/// Mensagem VICI: pares ordenados nome → valor.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Message {
    entries: Vec<(String, Value)>,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

impl Message {
    /// Mensagem vazia.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Acrescenta um par chave/valor.
    #[must_use]
    pub fn kv(mut self, key: &str, value: impl AsRef<[u8]>) -> Self {
        self.entries.push((
            key.to_owned(),
            Value::Str(Zeroizing::new(value.as_ref().to_vec())),
        ));
        self
    }

    /// Acrescenta uma lista.
    #[must_use]
    pub fn list<I, V>(mut self, key: &str, items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: AsRef<[u8]>,
    {
        let items = items
            .into_iter()
            .map(|v| Zeroizing::new(v.as_ref().to_vec()))
            .collect();
        self.entries.push((key.to_owned(), Value::List(items)));
        self
    }

    /// Acrescenta uma seção.
    #[must_use]
    pub fn section(mut self, key: &str, section: Message) -> Self {
        self.entries.push((key.to_owned(), Value::Section(section)));
        self
    }

    /// Itera sobre as entradas, na ordem.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Primeiro valor com essa chave.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Valor de texto (UTF-8) de uma chave.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    /// Seção de uma chave.
    #[must_use]
    pub fn get_section(&self, key: &str) -> Option<&Message> {
        match self.get(key)? {
            Value::Section(m) => Some(m),
            _ => None,
        }
    }

    /// Itens de texto de uma lista (itens não UTF-8 são ignorados).
    #[must_use]
    pub fn get_list(&self, key: &str) -> Vec<&str> {
        match self.get(key) {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|i| std::str::from_utf8(i).ok())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Seções de primeiro nível (ex.: uma por IKE_SA em `list-sa`).
    pub fn sections(&self) -> impl Iterator<Item = (&str, &Message)> {
        self.entries.iter().filter_map(|(k, v)| match v {
            Value::Section(m) => Some((k.as_str(), m)),
            _ => None,
        })
    }

    /// Codifica a mensagem, anexando a `out`.
    ///
    /// # Errors
    /// [`ViciError::Protocol`] se um nome passar de 255 bytes ou um valor de
    /// 65535 bytes (limites do formato).
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), ViciError> {
        for (key, value) in &self.entries {
            match value {
                Value::Str(bytes) => {
                    out.push(KEY_VALUE);
                    push_name(out, key)?;
                    push_value(out, bytes)?;
                }
                Value::Section(section) => {
                    out.push(SECTION_START);
                    push_name(out, key)?;
                    section.encode_into(out)?;
                    out.push(SECTION_END);
                }
                Value::List(items) => {
                    out.push(LIST_START);
                    push_name(out, key)?;
                    for item in items {
                        out.push(LIST_ITEM);
                        push_value(out, item)?;
                    }
                    out.push(LIST_END);
                }
            }
        }
        Ok(())
    }

    /// Decodifica uma mensagem completa.
    ///
    /// # Errors
    /// [`ViciError::Protocol`] se os bytes não formarem uma mensagem válida.
    pub fn decode(bytes: &[u8]) -> Result<Self, ViciError> {
        let mut reader = Reader { bytes, pos: 0 };
        let message = reader.message(0)?;
        if reader.pos != bytes.len() {
            return Err(protocol("SECTION_END sem SECTION_START"));
        }
        Ok(message)
    }
}

fn protocol(msg: &str) -> ViciError {
    ViciError::Protocol(msg.to_owned())
}

fn push_name(out: &mut Vec<u8>, name: &str) -> Result<(), ViciError> {
    let len = u8::try_from(name.len()).map_err(|_| protocol("nome VICI com mais de 255 bytes"))?;
    out.push(len);
    out.extend_from_slice(name.as_bytes());
    Ok(())
}

fn push_value(out: &mut Vec<u8>, value: &[u8]) -> Result<(), ViciError> {
    let len =
        u16::try_from(value.len()).map_err(|_| protocol("valor VICI com mais de 65535 bytes"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], ViciError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| protocol("mensagem VICI truncada"))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, ViciError> {
        Ok(self.take(1)?[0])
    }

    fn name(&mut self) -> Result<String, ViciError> {
        let len = usize::from(self.byte()?);
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| protocol("nome VICI não é UTF-8"))
    }

    fn value(&mut self) -> Result<Zeroizing<Vec<u8>>, ViciError> {
        let raw = self.take(2)?;
        let len = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
        Ok(Zeroizing::new(self.take(len)?.to_vec()))
    }

    /// Lê elementos até o fim dos bytes (depth 0) ou até `SECTION_END`.
    fn message(&mut self, depth: usize) -> Result<Message, ViciError> {
        if depth > MAX_DEPTH {
            return Err(protocol("seções VICI aninhadas demais"));
        }
        let mut message = Message::new();
        while self.pos < self.bytes.len() {
            match self.byte()? {
                KEY_VALUE => {
                    let key = self.name()?;
                    let value = self.value()?;
                    message.entries.push((key, Value::Str(value)));
                }
                SECTION_START => {
                    let key = self.name()?;
                    let section = self.message(depth + 1)?;
                    message.entries.push((key, Value::Section(section)));
                }
                SECTION_END => {
                    if depth == 0 {
                        // Devolve o byte para `decode` detectar o erro.
                        self.pos -= 1;
                    }
                    return Ok(message);
                }
                LIST_START => {
                    let key = self.name()?;
                    let mut items = Vec::new();
                    loop {
                        match self.byte()? {
                            LIST_ITEM => items.push(self.value()?),
                            LIST_END => break,
                            _ => return Err(protocol("elemento inesperado dentro de lista VICI")),
                        }
                    }
                    message.entries.push((key, Value::List(items)));
                }
                other => {
                    return Err(ViciError::Protocol(format!(
                        "elemento VICI desconhecido: {other}"
                    )));
                }
            }
        }
        if depth > 0 {
            return Err(protocol("seção VICI sem SECTION_END"));
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Message {
        Message::new()
            .kv("version", "1")
            .list("remote_addrs", ["192.0.2.1"])
            .section(
                "local-1",
                Message::new()
                    .kv("auth", "psk")
                    .kv("id", "keyid:grupo-teste"),
            )
            .section(
                "children",
                Message::new().section(
                    "rede",
                    Message::new().list("remote_ts", ["192.0.2.0/24", "198.51.100.0/24"]),
                ),
            )
    }

    #[test]
    fn roundtrip_preserves_order_and_nesting() {
        let mut bytes = Vec::new();
        sample().encode_into(&mut bytes).expect("encode");
        let decoded = Message::decode(&bytes).expect("decode");
        assert_eq!(decoded, sample());
        assert_eq!(decoded.get_str("version"), Some("1"));
        assert_eq!(decoded.get_list("remote_addrs"), ["192.0.2.1"]);
        let child = decoded
            .get_section("children")
            .and_then(|c| c.get_section("rede"))
            .expect("child");
        assert_eq!(child.get_list("remote_ts").len(), 2);
    }

    #[test]
    fn encoding_matches_wire_format() {
        let mut bytes = Vec::new();
        Message::new()
            .kv("a", "xy")
            .list("l", ["1"])
            .section("s", Message::new())
            .encode_into(&mut bytes)
            .expect("encode");
        assert_eq!(
            bytes,
            [
                3, 1, b'a', 0, 2, b'x', b'y', // KEY_VALUE
                4, 1, b'l', 5, 0, 1, b'1', 6, // LIST
                1, 1, b's', 2, // SECTION
            ]
        );
    }

    #[test]
    fn malformed_input_is_rejected() {
        let cases: &[&[u8]] = &[
            &[3, 5, b'a'],          // nome truncado
            &[3, 1, b'a', 0, 9, 1], // valor truncado
            &[1, 1, b's', 3],       // seção sem fim
            &[2],                   // fim sem início
            &[4, 1, b'l', 3],       // elemento estranho na lista
            &[9],                   // tipo desconhecido
            &[3, 1, 0xff, 0, 0],    // nome não UTF-8
        ];
        for (i, case) in cases.iter().enumerate() {
            assert!(Message::decode(case).is_err(), "caso {i} deveria falhar");
        }
    }

    #[test]
    fn deep_nesting_is_rejected() {
        let mut bytes = Vec::new();
        for _ in 0..=MAX_DEPTH + 1 {
            bytes.extend_from_slice(&[SECTION_START, 1, b's']);
        }
        bytes.extend(std::iter::repeat_n(SECTION_END, MAX_DEPTH + 2));
        assert!(Message::decode(&bytes).is_err());
    }

    #[test]
    fn oversized_fields_are_rejected_on_encode() {
        let long_name = "n".repeat(256);
        let mut out = Vec::new();
        assert!(
            Message::new()
                .kv(&long_name, "v")
                .encode_into(&mut out)
                .is_err()
        );
        let long_value = vec![b'v'; 65_536];
        assert!(
            Message::new()
                .kv("k", long_value)
                .encode_into(&mut out)
                .is_err()
        );
    }

    #[test]
    fn debug_never_shows_values() {
        let msg = Message::new()
            .kv("data", "segredo-super-secreto")
            .list("owners", ["usuario.teste"]);
        let debug = format!("{msg:?}");
        assert!(!debug.contains("segredo"), "{debug}");
        assert!(!debug.contains("usuario"), "{debug}");
        assert!(debug.contains("data"));
    }
}
