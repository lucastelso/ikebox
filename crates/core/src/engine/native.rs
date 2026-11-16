//! Cliente do `ipsec-client-helper` (engine embutido, ADR-0013/0014).
//!
//! Fala o protocolo de `ipsec-client-proto` pelo socket do helper. As
//! chamadas são bloqueantes (o `start` espera o engine ficar pronto) e, em
//! código async, rodam em `spawn_blocking`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use ipsec_client_proto::{
    EngineStatus, HELPER_SOCKET, MAX_LINE, Request, Response, StartParams, decode, encode,
};

use super::EngineError;

/// Tempo máximo de uma requisição (o `start` inclui a espera do engine).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Cliente do helper.
#[derive(Debug, Clone)]
pub struct NativeEngine {
    socket: PathBuf,
}

impl Default for NativeEngine {
    fn default() -> Self {
        Self::new(PathBuf::from(
            std::env::var_os("IPSEC_CLIENT_HELPER_SOCKET").unwrap_or_else(|| HELPER_SOCKET.into()),
        ))
    }
}

impl NativeEngine {
    /// Cliente para o socket indicado (testes); o padrão usa [`HELPER_SOCKET`]
    /// ou `$IPSEC_CLIENT_HELPER_SOCKET`.
    #[must_use]
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// O helper está instalado (o socket existe)?
    #[must_use]
    pub fn installed(&self) -> bool {
        self.socket.exists()
    }

    fn call(&self, request: &Request) -> Result<Response, EngineError> {
        let unavailable = |e: std::io::Error| {
            EngineError::Helper(format!(
                "componente de rede indisponível ({e}); reinstale o IPsec Client"
            ))
        };
        let mut stream = UnixStream::connect(&self.socket).map_err(unavailable)?;
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .map_err(unavailable)?;
        stream
            .set_write_timeout(Some(REQUEST_TIMEOUT))
            .map_err(unavailable)?;
        let line = encode(request).map_err(|e| EngineError::Helper(e.to_string()))?;
        stream.write_all(line.as_bytes()).map_err(unavailable)?;
        let mut reply = String::new();
        BufReader::new((&stream).take(MAX_LINE as u64 + 1))
            .read_line(&mut reply)
            .map_err(unavailable)?;
        match decode::<Response>(&reply).map_err(|e| EngineError::Helper(e.to_string()))? {
            Response::Error { message } => Err(EngineError::Helper(message)),
            other => Ok(other),
        }
    }

    fn unexpected(response: &Response) -> EngineError {
        EngineError::Helper(format!("resposta inesperada do helper: {response:?}"))
    }

    /// Versão do helper (também confirma que ele responde).
    ///
    /// # Errors
    /// Helper indisponível.
    pub fn ping(&self) -> Result<String, EngineError> {
        match self.call(&Request::Ping)? {
            Response::Pong { version } => Ok(version),
            other => Err(Self::unexpected(&other)),
        }
    }

    /// Sobe o engine e espera ficar pronto. Devolve o caminho do socket VICI.
    ///
    /// # Errors
    /// Recusa do helper (autorização, parâmetros, engine que não subiu).
    pub fn start(&self, params: &StartParams) -> Result<PathBuf, EngineError> {
        params.validate().map_err(EngineError::InvalidSpec)?;
        match self.call(&Request::Start(params.clone()))? {
            Response::Started { socket } => Ok(PathBuf::from(socket)),
            other => Err(Self::unexpected(&other)),
        }
    }

    /// Para o engine. Devolve se havia um rodando.
    ///
    /// # Errors
    /// Helper indisponível ou recusa.
    pub fn stop(&self) -> Result<bool, EngineError> {
        match self.call(&Request::Stop)? {
            Response::Stopped { was_running } => Ok(was_running),
            other => Err(Self::unexpected(&other)),
        }
    }

    /// Estado do engine.
    ///
    /// # Errors
    /// Helper indisponível ou recusa.
    pub fn status(&self) -> Result<EngineStatus, EngineError> {
        match self.call(&Request::Status)? {
            Response::Status(status) => Ok(status),
            other => Err(Self::unexpected(&other)),
        }
    }

    /// Últimas linhas do log do engine.
    ///
    /// # Errors
    /// Helper indisponível ou recusa.
    pub fn logs(&self, tail: u32) -> Result<Vec<String>, EngineError> {
        match self.call(&Request::Logs { tail })? {
            Response::Logs { lines } => Ok(lines),
            other => Err(Self::unexpected(&other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// Helper de mentira: responde uma linha fixa a cada conexão.
    fn fake_helper(reply: Response) -> (tempfile::TempDir, NativeEngine) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("helper.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let _ = (&stream).write_all(encode(&reply).expect("enc").as_bytes());
            }
        });
        (dir, NativeEngine::new(path))
    }

    #[test]
    fn maps_responses_and_errors() {
        let (_d, engine) = fake_helper(Response::Pong {
            version: "1.2.3".into(),
        });
        assert!(engine.installed());
        assert_eq!(engine.ping().expect("ping"), "1.2.3");

        let (_d, engine) = fake_helper(Response::Error {
            message: "não autorizado".into(),
        });
        let err = engine.stop().expect_err("erro");
        assert!(err.to_string().contains("não autorizado"), "{err}");

        let (_d, engine) = fake_helper(Response::Stopped { was_running: true });
        assert!(engine.status().is_err(), "resposta trocada é erro");
    }

    #[test]
    fn missing_helper_is_reported() {
        let engine = NativeEngine::new(PathBuf::from("/nao/existe.sock"));
        assert!(!engine.installed());
        assert!(matches!(engine.ping(), Err(EngineError::Helper(_))));
    }

    #[test]
    fn start_validates_before_calling() {
        let engine = NativeEngine::new(PathBuf::from("/nao/existe.sock"));
        let bad = StartParams {
            iface: "x;y".into(),
            if_id: 1,
            mtu: 1400,
            log_level: 1,
            profile_id: "p".into(),
        };
        assert!(matches!(
            engine.start(&bad),
            Err(EngineError::InvalidSpec(_))
        ));
    }
}
