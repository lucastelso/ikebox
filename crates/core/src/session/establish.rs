//! Etapa de negociação: carrega segredos e conexão no charon e inicia o túnel.
//!
//! Genérica sobre [`Charon`] para ser testada com um charon de mentira.

use std::time::Duration;

use super::SessionError;
use crate::profiles::{ConnParams, Profile};
use crate::secrets::Credentials;
use crate::vici::{Charon, ViciError};

/// Carrega os segredos (sempre antes da conexão), a conexão, e inicia o
/// child do perfil, esperando até `timeout`.
///
/// # Errors
/// Credenciais incompletas, perfil inválido ou recusa do charon. Uma falha
/// do `initiate` vira [`SessionError::Negotiation`] sem diagnóstico: quem
/// chama completa o diagnóstico com o log.
pub async fn establish<C: Charon>(
    charon: &mut C,
    profile: &Profile,
    credentials: &Credentials,
    iface: &str,
    if_id: u32,
    timeout: Duration,
) -> Result<(), SessionError> {
    let secrets = credentials.shared_secrets(profile)?;
    let conn = profile.to_load_conn(&ConnParams {
        iface,
        if_id,
        xauth_user: credentials.xauth_user(),
    })?;
    for secret in &secrets {
        charon.load_shared(secret).await?;
    }
    charon.load_conn(profile.conn_name(), &conn).await?;
    match charon
        .initiate(profile.conn_name(), &profile.child.name, timeout)
        .await
    {
        Ok(()) => Ok(()),
        Err(ViciError::CommandFailed { message, .. }) => Err(SessionError::Negotiation {
            kind: super::FailureKind::Unknown,
            detail: message,
        }),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::profiles::{CredentialKind, parse_profile};
    use crate::vici::{IkeSa, Message, SharedSecret, VersionInfo};
    use secrecy::SecretString;

    /// Charon de mentira: registra as chamadas e falha onde mandarem.
    #[derive(Default)]
    pub(crate) struct MockCharon {
        pub calls: Vec<String>,
        pub fail_initiate: Option<String>,
    }

    impl Charon for MockCharon {
        async fn version(&mut self) -> Result<VersionInfo, ViciError> {
            self.calls.push("version".into());
            Ok(VersionInfo::default())
        }
        async fn load_shared(&mut self, secret: &SharedSecret) -> Result<(), ViciError> {
            self.calls.push(format!("load-shared {}", secret.id));
            Ok(())
        }
        async fn unload_shared(&mut self, id: &str) -> Result<(), ViciError> {
            self.calls.push(format!("unload-shared {id}"));
            Ok(())
        }
        async fn load_conn(&mut self, name: &str, conn: &Message) -> Result<(), ViciError> {
            assert!(conn.get_section("children").is_some());
            self.calls.push(format!("load-conn {name}"));
            Ok(())
        }
        async fn unload_conn(&mut self, name: &str) -> Result<(), ViciError> {
            self.calls.push(format!("unload-conn {name}"));
            Ok(())
        }
        async fn list_conns(&mut self) -> Result<Vec<String>, ViciError> {
            Ok(Vec::new())
        }
        async fn initiate(&mut self, ike: &str, child: &str, _: Duration) -> Result<(), ViciError> {
            self.calls.push(format!("initiate {ike}/{child}"));
            match &self.fail_initiate {
                Some(msg) => Err(ViciError::CommandFailed {
                    command: "initiate".into(),
                    message: msg.clone(),
                }),
                None => Ok(()),
            }
        }
        async fn terminate(&mut self, ike: &str, _: Duration) -> Result<(), ViciError> {
            self.calls.push(format!("terminate {ike}"));
            Ok(())
        }
        async fn list_sas(&mut self) -> Result<Vec<IkeSa>, ViciError> {
            Ok(Vec::new())
        }
    }

    pub(crate) fn profile() -> Profile {
        parse_profile(include_str!(
            "../../../../presets/exemplo-ikev1-agressivo-xauth.json"
        ))
        .expect("preset")
    }

    pub(crate) fn credentials() -> Credentials {
        let mut c = Credentials::new();
        c.set(CredentialKind::Psk, SecretString::from("psk-falsa"));
        c.set(
            CredentialKind::XauthUser,
            SecretString::from("usuario.exemplo"),
        );
        c.set(
            CredentialKind::XauthPassword,
            SecretString::from("senha-falsa"),
        );
        c
    }

    #[tokio::test]
    async fn loads_secrets_before_connection_then_initiates() {
        let mut charon = MockCharon::default();
        establish(
            &mut charon,
            &profile(),
            &credentials(),
            "ipsc0",
            7,
            Duration::from_secs(1),
        )
        .await
        .expect("establish");
        assert_eq!(
            charon.calls,
            [
                "load-shared ike-exemplo-legado",
                "load-shared xauth-exemplo-legado",
                "load-conn exemplo-legado",
                "initiate exemplo-legado/rede-interna",
            ]
        );
    }

    #[tokio::test]
    async fn missing_credentials_stop_before_touching_charon() {
        let mut charon = MockCharon::default();
        let err = establish(
            &mut charon,
            &profile(),
            &Credentials::new(),
            "ipsc0",
            7,
            Duration::from_secs(1),
        )
        .await
        .expect_err("deve falhar");
        assert!(matches!(err, SessionError::Secrets(_)));
        assert!(charon.calls.is_empty());
    }

    #[tokio::test]
    async fn initiate_failure_becomes_negotiation_error() {
        let mut charon = MockCharon {
            fail_initiate: Some("establishing CHILD_SA 'rede-interna' failed".into()),
            ..Default::default()
        };
        let err = establish(
            &mut charon,
            &profile(),
            &credentials(),
            "ipsc0",
            7,
            Duration::from_secs(1),
        )
        .await
        .expect_err("deve falhar");
        assert!(matches!(err, SessionError::Negotiation { .. }));
    }
}
