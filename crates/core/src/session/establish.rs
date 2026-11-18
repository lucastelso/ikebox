//! Negotiation step: loads the secrets and the connection into charon and
//! starts the tunnel.
//!
//! Generic over [`Charon`] so it can be tested with a fake charon.

use std::time::Duration;

use super::SessionError;
use crate::profiles::{ConnParams, Profile};
use crate::secrets::{AgentPublicKey, Credentials};
use crate::vici::{Charon, ViciError};

/// Loads the secrets (always before the connection), the connection, and
/// starts the profile's child, waiting up to `timeout`.
///
/// # Errors
/// Incomplete credentials, invalid profile or charon's refusal. An `initiate`
/// failure becomes [`SessionError::Negotiation`] without a diagnosis: the
/// caller completes the diagnosis with the log.
pub async fn establish<C: Charon>(
    charon: &mut C,
    profile: &Profile,
    credentials: &Credentials,
    iface: &str,
    if_id: u32,
    updown: Option<&str>,
    timeout: Duration,
) -> Result<(), SessionError> {
    let secrets = credentials.shared_secrets(profile)?;
    let conn = profile.to_load_conn(&ConnParams {
        iface,
        updown,
        if_id,
        xauth_user: credentials.xauth_user(),
        eap_identity: credentials.eap_identity(),
        client_cert: credentials.client_cert_pem(),
    })?;
    for secret in &secrets {
        charon.load_shared(secret).await?;
    }
    if profile.auth.uses_token() {
        // The key stays on the token: charon asks the key agent of this
        // process to sign (ADR-0029), so only the public key goes.
        let cert = credentials
            .client_cert_pem()
            .ok_or(crate::secrets::SecretsError::Missing(
                crate::profiles::CredentialKind::ClientCert,
            ))?;
        let public = AgentPublicKey::from_cert_pem(cert)?;
        charon.load_agent_key(&public.spki_pem).await?;
    } else if let Some(key) = credentials.private_key() {
        charon.load_key(&key).await?;
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
    use crate::vici::{IkeSa, Message, PrivateKey, SharedSecret, VersionInfo};
    use secrecy::SecretString;

    /// Fake charon: records the calls and fails where told.
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
        async fn load_key(&mut self, _key: &PrivateKey) -> Result<(), ViciError> {
            self.calls.push("load-key".into());
            Ok(())
        }
        async fn load_agent_key(&mut self, pem: &str) -> Result<(), ViciError> {
            assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
            self.calls.push("load-key agent".into());
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
            "../../../../presets/example-ikev1-aggressive-xauth.json"
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
            Some(crate::profiles::UPDOWN_PATH),
            Duration::from_secs(1),
        )
        .await
        .expect("establish");
        assert_eq!(
            charon.calls,
            [
                "load-shared ike-example-legacy",
                "load-shared xauth-example-legacy",
                "load-conn example-legacy",
                "initiate example-legacy/internal-network",
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
            Some(crate::profiles::UPDOWN_PATH),
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
            fail_initiate: Some("establishing CHILD_SA 'internal-network' failed".into()),
            ..Default::default()
        };
        let err = establish(
            &mut charon,
            &profile(),
            &credentials(),
            "ipsc0",
            7,
            Some(crate::profiles::UPDOWN_PATH),
            Duration::from_secs(1),
        )
        .await
        .expect_err("deve falhar");
        assert!(matches!(err, SessionError::Negotiation { .. }));
    }
}
