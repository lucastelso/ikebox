//! VICI transport over a Unix socket: packets, the command connection and the
//! event connection (ADR-0003, ADR-0011).
//!
//! Packet: a `u32` (big-endian) with the length + payload. Payload: type
//! (`u8`), name (`u8` + bytes) in the named types, and the message.
//!
//! We use **two connections**, as the VICI documentation recommends: one for
//! commands (responses in order) and another only for subscribed events.

use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::trace;
use zeroize::Zeroizing;

use super::{Message, ViciError};

/// Maximum packet size accepted (the same default limit as charon).
pub const MAX_PACKET: usize = 512 * 1024;

const CMD_REQUEST: u8 = 0;
const CMD_RESPONSE: u8 = 1;
const CMD_UNKNOWN: u8 = 2;
const EVENT_REGISTER: u8 = 3;
const EVENT_UNREGISTER: u8 = 4;
const EVENT_CONFIRM: u8 = 5;
const EVENT_UNKNOWN: u8 = 6;
const EVENT: u8 = 7;

/// A received packet.
#[derive(Debug)]
enum Packet {
    Response(Message),
    UnknownCommand,
    EventConfirm,
    UnknownEvent,
    Event(String, Message),
}

async fn write_packet(
    stream: &mut UnixStream,
    kind: u8,
    name: Option<&str>,
    message: Option<&Message>,
) -> Result<(), ViciError> {
    // Buffer zeroed on drop: it may hold a PSK or password (load-shared).
    let mut payload = Zeroizing::new(vec![kind]);
    if let Some(name) = name {
        let len = u8::try_from(name.len()).map_err(|_| {
            ViciError::Protocol(
                crate::tr!(
                    "command/event name too long",
                    "nome de comando/evento longo demais"
                )
                .to_owned(),
            )
        })?;
        payload.push(len);
        payload.extend_from_slice(name.as_bytes());
    }
    if let Some(message) = message {
        message.encode_into(&mut payload)?;
    }
    if payload.len() > MAX_PACKET {
        return Err(ViciError::TooLarge(payload.len()));
    }
    let len = u32::try_from(payload.len()).map_err(|_| ViciError::TooLarge(payload.len()))?;
    let mut frame = Zeroizing::new(Vec::with_capacity(4 + payload.len()));
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    stream.write_all(&frame).await?;
    Ok(())
}

async fn read_packet(stream: &mut UnixStream) -> Result<Packet, ViciError> {
    let mut len = [0u8; 4];
    match stream.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Err(ViciError::Closed),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 {
        return Err(ViciError::Protocol("pacote VICI vazio".to_owned()));
    }
    if len > MAX_PACKET {
        return Err(ViciError::TooLarge(len));
    }
    let mut payload = Zeroizing::new(vec![0u8; len]);
    stream.read_exact(&mut payload).await?;
    parse_packet(&payload)
}

fn parse_packet(payload: &[u8]) -> Result<Packet, ViciError> {
    let (&kind, rest) = payload
        .split_first()
        .ok_or_else(|| ViciError::Protocol("pacote VICI vazio".to_owned()))?;
    match kind {
        CMD_RESPONSE => Ok(Packet::Response(Message::decode(rest)?)),
        CMD_UNKNOWN => Ok(Packet::UnknownCommand),
        EVENT_CONFIRM => Ok(Packet::EventConfirm),
        EVENT_UNKNOWN => Ok(Packet::UnknownEvent),
        EVENT => {
            let (&name_len, rest) = rest.split_first().ok_or_else(|| {
                ViciError::Protocol(
                    crate::tr!("VICI event without a name", "evento VICI sem nome").to_owned(),
                )
            })?;
            let name_len = usize::from(name_len);
            if rest.len() < name_len {
                return Err(ViciError::Protocol(
                    crate::tr!("truncated event name", "nome de evento truncado").to_owned(),
                ));
            }
            let (name, body) = rest.split_at(name_len);
            let name = String::from_utf8(name.to_vec()).map_err(|_| {
                ViciError::Protocol(
                    crate::tr!("event name is not UTF-8", "nome de evento não é UTF-8").to_owned(),
                )
            })?;
            Ok(Packet::Event(name, Message::decode(body)?))
        }
        other => Err(ViciError::Protocol(crate::trf!(
            "unexpected VICI packet type: {other}",
            "tipo de pacote VICI inesperado: {other}"
        ))),
    }
}

/// Command connection to charon.
#[derive(Debug)]
pub struct ViciClient {
    stream: UnixStream,
}

impl ViciClient {
    /// Connects to the VICI socket.
    ///
    /// # Errors
    /// I/O error (socket missing, permission denied...).
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, ViciError> {
        Ok(Self {
            stream: UnixStream::connect(path).await?,
        })
    }

    /// Sends a command and returns the response.
    ///
    /// # Errors
    /// [`ViciError::UnknownCommand`], protocol or I/O errors. It does not
    /// interpret `success = no`: that is left to [`super::Charon`].
    pub async fn request(
        &mut self,
        command: &str,
        message: &Message,
    ) -> Result<Message, ViciError> {
        trace!(command, "VICI request");
        write_packet(&mut self.stream, CMD_REQUEST, Some(command), Some(message)).await?;
        match read_packet(&mut self.stream).await? {
            Packet::Response(response) => Ok(response),
            Packet::UnknownCommand => Err(ViciError::UnknownCommand(command.to_owned())),
            other => Err(ViciError::Protocol(format!(
                "resposta inesperada a '{command}': {other:?}"
            ))),
        }
    }

    /// Command with a streamed response (e.g. `list-sas` → `list-sa` events):
    /// subscribes to the event, sends the command, collects the events until the
    /// final response and unsubscribes.
    ///
    /// # Errors
    /// As [`Self::request`], plus [`ViciError::UnknownEvent`].
    pub async fn streamed_request(
        &mut self,
        command: &str,
        event: &str,
        message: &Message,
    ) -> Result<(Vec<Message>, Message), ViciError> {
        self.event_control(EVENT_REGISTER, event).await?;
        let result = async {
            write_packet(&mut self.stream, CMD_REQUEST, Some(command), Some(message)).await?;
            let mut events = Vec::new();
            loop {
                match read_packet(&mut self.stream).await? {
                    Packet::Event(name, msg) if name == event => events.push(msg),
                    Packet::Event(..) => {}
                    Packet::Response(response) => return Ok((events, response)),
                    Packet::UnknownCommand => {
                        return Err(ViciError::UnknownCommand(command.to_owned()));
                    }
                    other => {
                        return Err(ViciError::Protocol(format!(
                            "pacote inesperado em '{command}': {other:?}"
                        )));
                    }
                }
            }
        }
        .await;
        self.event_control(EVENT_UNREGISTER, event).await?;
        result
    }

    async fn event_control(&mut self, kind: u8, event: &str) -> Result<(), ViciError> {
        write_packet(&mut self.stream, kind, Some(event), None).await?;
        loop {
            match read_packet(&mut self.stream).await? {
                Packet::EventConfirm => return Ok(()),
                Packet::UnknownEvent => return Err(ViciError::UnknownEvent(event.to_owned())),
                // A late event of the previous stream may arrive before the confirmation.
                Packet::Event(..) => {}
                other => {
                    return Err(ViciError::Protocol(crate::trf!(
                        "unexpected event confirmation: {other:?}",
                        "confirmação de evento inesperada: {other:?}"
                    )));
                }
            }
        }
    }
}

/// Connection dedicated to subscribed events (`ike-updown`, `child-updown`,
/// `log`).
#[derive(Debug)]
pub struct ViciEvents {
    stream: UnixStream,
}

impl ViciEvents {
    /// Connects and subscribes to the given events.
    ///
    /// # Errors
    /// I/O or protocol error, or an unknown event.
    pub async fn subscribe(path: impl AsRef<Path>, events: &[&str]) -> Result<Self, ViciError> {
        let mut stream = UnixStream::connect(path).await?;
        for event in events {
            write_packet(&mut stream, EVENT_REGISTER, Some(event), None).await?;
            match read_packet(&mut stream).await? {
                Packet::EventConfirm => {}
                Packet::UnknownEvent => return Err(ViciError::UnknownEvent((*event).to_owned())),
                other => {
                    return Err(ViciError::Protocol(crate::trf!(
                        "unexpected event confirmation: {other:?}",
                        "confirmação de evento inesperada: {other:?}"
                    )));
                }
            }
        }
        Ok(Self { stream })
    }

    /// Next event (name, message).
    ///
    /// # Errors
    /// [`ViciError::Closed`] when charon closes the connection (e.g. engine
    /// stopped), or protocol or I/O errors.
    pub async fn next(&mut self) -> Result<(String, Message), ViciError> {
        loop {
            if let Packet::Event(name, message) = read_packet(&mut self.stream).await? {
                return Ok((name, message));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_packet() {
        let mut payload = vec![EVENT, 3, b'l', b'o', b'g'];
        Message::new()
            .kv("msg", "olá")
            .encode_into(&mut payload)
            .expect("encode");
        match parse_packet(&payload).expect("parse") {
            Packet::Event(name, msg) => {
                assert_eq!(name, "log");
                assert_eq!(msg.get_str("msg"), Some("olá"));
            }
            other => panic!("pacote errado: {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_packets() {
        assert!(parse_packet(&[]).is_err());
        assert!(parse_packet(&[EVENT]).is_err());
        assert!(parse_packet(&[EVENT, 5, b'a']).is_err());
        assert!(parse_packet(&[42]).is_err());
        assert!(matches!(
            parse_packet(&[CMD_UNKNOWN]),
            Ok(Packet::UnknownCommand)
        ));
    }

    /// Fake VICI server on a socketpair: checks the `streamed_request` flow
    /// (register → events → response → unregister).
    #[tokio::test]
    async fn streamed_request_flow() {
        let (client_side, mut server) = UnixStream::pair().expect("socketpair");
        let mut client = ViciClient {
            stream: client_side,
        };

        let server_task = tokio::spawn(async move {
            let expect = |want: u8, buf: &[u8]| assert_eq!(buf[4], want);
            let mut buf = vec![0u8; 256];

            let n = server.read(&mut buf).await.expect("register");
            expect(EVENT_REGISTER, &buf[..n]);
            server
                .write_all(&[0, 0, 0, 1, EVENT_CONFIRM])
                .await
                .expect("w");

            let n = server.read(&mut buf).await.expect("cmd");
            expect(CMD_REQUEST, &buf[..n]);
            for i in 0..2u8 {
                let mut p = vec![EVENT, 7];
                p.extend_from_slice(b"list-sa");
                Message::new()
                    .kv("i", [b'0' + i])
                    .encode_into(&mut p)
                    .expect("enc");
                let mut frame = (p.len() as u32).to_be_bytes().to_vec();
                frame.extend_from_slice(&p);
                server.write_all(&frame).await.expect("w");
            }
            let mut p = vec![CMD_RESPONSE];
            Message::new()
                .kv("success", "yes")
                .encode_into(&mut p)
                .expect("enc");
            let mut frame = (p.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&p);
            server.write_all(&frame).await.expect("w");

            let n = server.read(&mut buf).await.expect("unregister");
            expect(EVENT_UNREGISTER, &buf[..n]);
            server
                .write_all(&[0, 0, 0, 1, EVENT_CONFIRM])
                .await
                .expect("w");
        });

        let (events, response) = client
            .streamed_request("list-sas", "list-sa", &Message::new())
            .await
            .expect("streamed");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].get_str("i"), Some("1"));
        assert_eq!(response.get_str("success"), Some("yes"));
        server_task.await.expect("servidor");
    }
}
