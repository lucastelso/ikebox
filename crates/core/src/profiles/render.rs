//! Geração da configuração do charon a partir do perfil: a mensagem
//! `load-conn` (VICI) e o comando do `updown` (ADR-0004, ADR-0008).

use sha2::{Digest, Sha256};

use super::ProfileError;
use super::model::{AuthMethod, ChildAction, Profile};
use super::validate::{is_identity, validate};
use crate::vici::Message;

/// Caminho do `updown` na imagem Docker do engine. No engine embutido, o
/// comando do `updown` é fixado pelo root e o que vier pelo VICI é ignorado
/// (ADR-0019): lá, o `load-conn` vai sem `updown`.
pub const UPDOWN_PATH: &str = "/usr/local/libexec/ipsec-client/updown";

/// Dados da sessão necessários para gerar a conexão.
#[derive(Debug, Clone, Copy)]
pub struct ConnParams<'a> {
    /// Interface XFRM do engine.
    pub iface: &'a str,
    /// Caminho do `updown` a mandar no `load-conn` ([`UPDOWN_PATH`] no
    /// engine Docker; `None` no embutido, onde o root o define).
    pub updown: Option<&'a str>,
    /// `if_id` das SAs/políticas.
    pub if_id: u32,
    /// Usuário XAuth (obrigatório em `psk-xauth`). Não é segredo, mas é
    /// identificador pessoal: vem das credenciais, nunca do perfil.
    pub xauth_user: Option<&'a str>,
}

impl Profile {
    /// Nome da conexão no charon.
    #[must_use]
    pub fn conn_name(&self) -> &str {
        &self.id
    }

    /// `if_id` estável derivado do id do perfil (1..=2³¹-1, nunca 0).
    #[must_use]
    pub fn if_id(&self) -> u32 {
        let digest = Sha256::digest(self.id.as_bytes());
        let raw = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) & 0x7fff_ffff;
        raw.max(1)
    }

    /// Comando do `updown` (executado pelo charon via `/bin/sh -c`): cada
    /// argumento vai entre aspas simples, e os valores já foram validados.
    ///
    /// # Errors
    /// [`ProfileError::Unsafe`] se algum valor não puder ir entre aspas simples
    /// (defesa extra; a validação já recusa esses valores).
    pub fn updown_command(&self, updown: &str, iface: &str) -> Result<String, ProfileError> {
        let dns = &self.network.dns;
        if !updown.starts_with('/') || updown.chars().any(|c| c.is_whitespace() || c == '\'') {
            return Err(ProfileError::Unsafe(
                "caminho do updown inválido".to_owned(),
            ));
        }
        let mut cmd = format!("{updown} --iface {}", shell_quote(iface)?);
        for server in &dns.servers {
            cmd.push_str(&format!(" --dns {}", shell_quote(&server.to_string())?));
        }
        for domain in &dns.domains {
            cmd.push_str(&format!(" --domain {}", shell_quote(domain)?));
        }
        if dns.use_received {
            cmd.push_str(" --use-received-dns");
        }
        if dns.default_route {
            cmd.push_str(" --dns-default-route");
        }
        Ok(cmd)
    }

    /// Mensagem `load-conn` (o conteúdo da seção `<conn_name>`).
    ///
    /// # Errors
    /// Perfil inválido ([`ProfileError::Invalid`]), usuário XAuth ausente ou
    /// inválido ([`ProfileError::MissingCredential`]).
    pub fn to_load_conn(&self, params: &ConnParams<'_>) -> Result<Message, ProfileError> {
        let issues = validate(self);
        if !issues.is_empty() {
            return Err(ProfileError::Invalid(issues));
        }
        let ike = &self.ike;
        let mut conn = Message::new()
            .list("remote_addrs", [&self.gateway])
            .kv("version", ike.version.to_string())
            .kv("aggressive", yes_no(ike.aggressive))
            .kv("encap", yes_no(ike.encap))
            .list("proposals", &ike.proposals);
        if let Some(secs) = ike.rekey_time_secs {
            conn = conn.kv("rekey_time", format!("{secs}s"));
        }
        if let Some(secs) = ike.dpd_delay_secs {
            conn = conn.kv("dpd_delay", format!("{secs}s"));
        }
        if self.network.request_vip {
            conn = conn.list("vips", ["0.0.0.0"]);
        }

        let mut local = Message::new().kv("auth", "psk");
        if let Some(id) = &self.auth.local_id {
            local = local.kv("id", id);
        }
        conn = conn.section("local-1", local);
        if self.auth.method == AuthMethod::PskXauth {
            let user = params
                .xauth_user
                .filter(|u| is_identity(u))
                .ok_or(ProfileError::MissingCredential("usuário XAuth"))?;
            conn = conn.section(
                "local-2",
                Message::new().kv("auth", "xauth").kv("xauth_id", user),
            );
        }
        let mut remote = Message::new().kv("auth", "psk");
        if let Some(id) = &self.auth.remote_id {
            remote = remote.kv("id", id);
        }
        conn = conn.section("remote", remote);

        let c = &self.child;
        let mut child = Message::new()
            .list("remote_ts", &c.remote_ts)
            .list("esp_proposals", &c.esp_proposals)
            .kv("start_action", "none")
            .kv("dpd_action", dpd_action(c.dpd_action))
            .kv("close_action", close_action(c.close_action))
            .kv("if_id_in", params.if_id.to_string())
            .kv("if_id_out", params.if_id.to_string());
        if let Some(updown) = params.updown {
            child = child.kv("updown", self.updown_command(updown, params.iface)?);
        }
        if !c.local_ts.is_empty() {
            child = child.list("local_ts", &c.local_ts);
        }
        if let Some(secs) = c.rekey_time_secs {
            child = child.kv("rekey_time", format!("{secs}s"));
        }
        Ok(conn.section("children", Message::new().section(&c.name, child)))
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn dpd_action(a: ChildAction) -> &'static str {
    match a {
        ChildAction::None => "clear",
        ChildAction::Trap => "trap",
        ChildAction::Restart => "restart",
    }
}

fn close_action(a: ChildAction) -> &'static str {
    match a {
        ChildAction::None => "none",
        ChildAction::Trap => "trap",
        ChildAction::Restart => "restart",
    }
}

/// Aspas simples para `/bin/sh`. Recusa aspas simples e caracteres de controle
/// em vez de escapar: valores assim nunca deveriam chegar aqui.
fn shell_quote(value: &str) -> Result<String, ProfileError> {
    if value.is_empty() || value.contains('\'') || value.chars().any(char::is_control) {
        return Err(ProfileError::Unsafe(
            "valor impróprio para o comando do updown".to_owned(),
        ));
    }
    Ok(format!("'{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_rejects_breakouts() {
        assert_eq!(shell_quote("~corp").ok().as_deref(), Some("'~corp'"));
        for bad in ["", "a'b", "a\nb", "'; rm -rf / #"] {
            assert!(shell_quote(bad).is_err(), "{bad:?}");
        }
    }
}
