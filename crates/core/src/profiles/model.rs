//! Connection profile model (ADR-0008, ADR-0022), serialized as JSON.
//!
//! A profile describes the VPN (gateway, proposals, routes, DNS) and **never**
//! contains credentials: no PSK, no XAuth username or password (ADR-0007). So it
//! can be handed to colleagues, and each person enters their own credentials.
//!
//! In memory the profile has the shape of format 2 (authentication rounds).
//! Format 1 is read and converted, and the profile is saved in the lowest
//! format that expresses it (see `schema.rs`).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Newest version of the profile format.
pub const SCHEMA_VERSION: u32 = 2;

/// Connection profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Identifier (`[a-z0-9-]`): name of the file, of the connection in charon and
    /// key in the keyring.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Gateway: DNS name or IP.
    pub gateway: String,
    /// Phase 1.
    pub ike: IkeSettings,
    /// Authentication.
    pub auth: AuthSettings,
    /// Phase 2.
    pub child: ChildSettings,
    /// Network and DNS.
    #[serde(default)]
    pub network: NetworkSettings,
}

/// Phase 1 (IKE_SA).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IkeSettings {
    /// IKE version: 1 or 2.
    pub version: u8,
    /// Aggressive mode (IKEv1 only).
    #[serde(default)]
    pub aggressive: bool,
    /// Forces UDP encapsulation (NAT-T) even without NAT.
    #[serde(default)]
    pub encap: bool,
    /// IKE proposals (e.g. `aes256-sha1-modp1024`), in order of preference.
    pub proposals: Vec<String>,
    /// IKE_SA rekey interval, in seconds (missing = charon's default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rekey_time_secs: Option<u32>,
    /// DPD interval, in seconds (missing = no DPD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpd_delay_secs: Option<u32>,
}

/// Method of a client authentication round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalMethod {
    /// Pre-shared key (often the group's).
    Psk,
    /// XAuth username and password (IKEv1, second round).
    Xauth,
    /// Username and password through EAP-MSCHAPv2 (IKEv2; the gateway
    /// authenticates with a certificate, ADR-0023).
    EapMschapv2,
    /// Client certificate (IKEv2, or IKEv1 in main mode): the key and certificate
    /// come from the keyring, never from the profile (ADR-0024).
    Certificate,
}

/// A client authentication round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRound {
    /// Method.
    pub method: LocalMethod,
    /// Identity sent in this round (e.g. `keyid:group-name` in the PSK round).
    /// Missing = the computer's address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The password is a one-time code: it is never stored in the keyring and is
    /// asked on every connection (XAuth only; ADR-0025).
    #[serde(default, skip_serializing_if = "is_false")]
    pub otp: bool,
    /// The certificate's private key is on a smartcard or token, used
    /// through the person's own process (ADR-0029), not imported into the
    /// keyring. Only for the certificate round.
    #[serde(default, skip_serializing_if = "is_false")]
    pub token: bool,
}

impl LocalRound {
    /// Round with neither identity nor OTP.
    #[must_use]
    pub fn new(method: LocalMethod) -> Self {
        Self {
            method,
            id: None,
            otp: false,
            token: false,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // signature required by serde
fn is_false(value: &bool) -> bool {
    !*value
}

/// How the gateway proves who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteMethod {
    /// The same PSK as the client.
    Psk,
    /// Certificate, checked against the profile's CA (`ca`) and the identity
    /// `id` (ADR-0022): never "accept any certificate".
    Certificate,
}

/// Gateway authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAuth {
    /// Method.
    pub method: RemoteMethod,
    /// Identity the gateway must present (missing = any; required with a
    /// certificate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// CAs (PEM) the gateway's certificate must chain to. Public data, sent to
    /// charon as data (never as a path).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ca: Vec<String>,
}

/// Authentication: the client's rounds, in order, and the gateway's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettings {
    /// The client's rounds (1 or 2).
    pub local: Vec<LocalRound>,
    /// Gateway.
    pub remote: RemoteAuth,
}

impl AuthSettings {
    /// PSK (with the identity `psk_id`), optionally followed by XAuth, and the
    /// gateway authenticated by the same PSK.
    #[must_use]
    pub fn psk(psk_id: Option<String>, xauth: bool, remote_id: Option<String>) -> Self {
        let mut local = vec![LocalRound {
            id: psk_id,
            ..LocalRound::new(LocalMethod::Psk)
        }];
        if xauth {
            local.push(LocalRound::new(LocalMethod::Xauth));
        }
        Self {
            local,
            remote: RemoteAuth {
                method: RemoteMethod::Psk,
                id: remote_id,
                ca: Vec::new(),
            },
        }
    }

    /// Username and password through EAP-MSCHAPv2, with the gateway authenticated
    /// by a certificate for the identity `gateway_id`, issued by one of the `ca`.
    #[must_use]
    pub fn eap(local_id: Option<String>, gateway_id: String, ca: Vec<String>) -> Self {
        Self {
            local: vec![LocalRound {
                id: local_id,
                ..LocalRound::new(LocalMethod::EapMschapv2)
            }],
            remote: RemoteAuth {
                method: RemoteMethod::Certificate,
                id: Some(gateway_id),
                ca,
            },
        }
    }

    /// Is there an EAP round?
    #[must_use]
    pub fn uses_eap(&self) -> bool {
        self.round(LocalMethod::EapMschapv2).is_some()
    }

    /// Does the person authenticate with a certificate?
    #[must_use]
    pub fn uses_client_certificate(&self) -> bool {
        self.round(LocalMethod::Certificate).is_some()
    }

    /// Is the certificate's key on a smartcard or token (ADR-0029)?
    #[must_use]
    pub fn uses_token(&self) -> bool {
        self.round(LocalMethod::Certificate)
            .is_some_and(|r| r.token)
    }

    /// Client certificate (with optional XAuth in IKEv1), and the gateway
    /// authenticated by its certificate, for the identity `gateway_id` and issued
    /// by one of the `ca`.
    #[must_use]
    pub fn certificate(
        local_id: Option<String>,
        xauth: bool,
        gateway_id: Option<String>,
        ca: Vec<String>,
    ) -> Self {
        let mut local = vec![LocalRound {
            id: local_id,
            ..LocalRound::new(LocalMethod::Certificate)
        }];
        if xauth {
            local.push(LocalRound::new(LocalMethod::Xauth));
        }
        Self {
            local,
            remote: RemoteAuth {
                method: RemoteMethod::Certificate,
                id: gateway_id,
                ca,
            },
        }
    }

    fn round(&self, method: LocalMethod) -> Option<&LocalRound> {
        self.local.iter().find(|r| r.method == method)
    }

    /// Identity of the PSK round (e.g. the group id).
    #[must_use]
    pub fn psk_id(&self) -> Option<&str> {
        self.round(LocalMethod::Psk).and_then(|r| r.id.as_deref())
    }

    /// Identity expected from the gateway.
    #[must_use]
    pub fn remote_id(&self) -> Option<&str> {
        self.remote.id.as_deref()
    }

    /// Is there an XAuth round?
    #[must_use]
    pub fn uses_xauth(&self) -> bool {
        self.round(LocalMethod::Xauth).is_some()
    }

    /// Is the XAuth password one-time?
    #[must_use]
    pub fn otp(&self) -> bool {
        self.round(LocalMethod::Xauth).is_some_and(|r| r.otp)
    }
}

/// CHILD_SA action after DPD or when the gateway closes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildAction {
    /// Does nothing (the tunnel goes down).
    #[default]
    None,
    /// Installs a trap policy: reconnects when there is traffic.
    Trap,
    /// Reconnects immediately.
    Restart,
}

/// Phase 2 (CHILD_SA).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildSettings {
    /// Child name (`[a-z0-9-]`).
    #[serde(default = "default_child_name")]
    pub name: String,
    /// ESP proposals, in order of preference (including the DH group enables PFS).
    pub esp_proposals: Vec<String>,
    /// CHILD_SA rekey interval, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rekey_time_secs: Option<u32>,
    /// Remote networks reached through the tunnel (CIDR or IP). `0.0.0.0/0` = full
    /// tunnel.
    pub remote_ts: Vec<String>,
    /// Local networks (empty = the virtual IP received).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_ts: Vec<String>,
    /// Action after DPD finds the gateway unreachable.
    #[serde(default)]
    pub dpd_action: ChildAction,
    /// Action when the gateway closes the tunnel.
    #[serde(default)]
    pub close_action: ChildAction,
}

fn default_child_name() -> String {
    "rede".to_owned()
}

/// Network and DNS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NetworkSettings {
    /// Asks the gateway for a virtual IP (mode config).
    pub request_vip: bool,
    /// MTU of the tunnel interface.
    pub mtu: u16,
    /// DNS.
    pub dns: DnsSettings,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            request_vip: true,
            mtu: 1400,
            dns: DnsSettings::default(),
        }
    }
}

/// DNS of the tunnel interface (systemd-resolved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsSettings {
    /// Fixed DNS servers.
    pub servers: Vec<IpAddr>,
    /// Domains: `~domain` only routes queries; `domain` also goes into the search
    /// list; `~.` sends every query to these servers.
    pub domains: Vec<String>,
    /// Also use the servers sent by the gateway.
    pub use_received: bool,
    /// Use these servers for names outside the domains.
    pub default_route: bool,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            domains: Vec::new(),
            use_received: true,
            default_route: false,
        }
    }
}

/// Credential the profile requires from the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialKind {
    /// PSK (usually the group's).
    Psk,
    /// XAuth username (e.g. employee id).
    XauthUser,
    /// XAuth password.
    XauthPassword,
    /// EAP identity (the username; not a secret, but personal).
    EapIdentity,
    /// EAP password.
    EapPassword,
    /// Client certificate (PEM): public, but with the person's name.
    ClientCert,
    /// The client's private key (PEM, PKCS#8).
    ClientKey,
    /// Where the key is on a smartcard or token (ADR-0029): the PKCS#11
    /// module and the key's identifier. Public, stored like the certificate.
    TokenKey,
    /// PIN of the smartcard or token: asked at every connection, never
    /// stored.
    TokenPin,
}

impl CredentialKind {
    /// Stable name (keyring key: `<profile>:<field>`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Psk => "psk",
            Self::XauthUser => "xauth-user",
            Self::XauthPassword => "xauth-password",
            Self::EapIdentity => "eap-identity",
            Self::EapPassword => "eap-password",
            Self::ClientCert => "client-cert",
            Self::ClientKey => "client-key",
            Self::TokenKey => "token-key",
            Self::TokenPin => "token-pin",
        }
    }

    /// Is the value secret (never shown)? The XAuth username is not a secret, but
    /// it is masked when shown.
    #[must_use]
    pub fn is_secret(self) -> bool {
        !matches!(
            self,
            Self::XauthUser | Self::EapIdentity | Self::ClientCert | Self::TokenKey
        )
    }

    /// Typed by the person (as opposed to imported from a certificate file).
    #[must_use]
    pub fn is_typed(self) -> bool {
        !matches!(self, Self::ClientCert | Self::ClientKey | Self::TokenKey)
    }
}

impl Profile {
    /// Credentials needed to connect, in the order of the rounds.
    #[must_use]
    pub fn required_credentials(&self) -> Vec<CredentialKind> {
        let mut kinds = Vec::new();
        for round in &self.auth.local {
            match round.method {
                LocalMethod::Psk => kinds.push(CredentialKind::Psk),
                LocalMethod::Xauth => {
                    kinds.extend([CredentialKind::XauthUser, CredentialKind::XauthPassword]);
                }
                LocalMethod::EapMschapv2 => {
                    kinds.extend([CredentialKind::EapIdentity, CredentialKind::EapPassword]);
                }
                LocalMethod::Certificate if round.token => {
                    kinds.extend([
                        CredentialKind::ClientCert,
                        CredentialKind::TokenKey,
                        CredentialKind::TokenPin,
                    ]);
                }
                LocalMethod::Certificate => {
                    kinds.extend([CredentialKind::ClientCert, CredentialKind::ClientKey]);
                }
            }
        }
        kinds
    }

    /// Credentials that may stay in the keyring: all but the one-time password
    /// (`otp`) and the token PIN, asked on every connection.
    #[must_use]
    pub fn storable_credentials(&self) -> Vec<CredentialKind> {
        let otp = self.auth.otp();
        self.required_credentials()
            .into_iter()
            .filter(|kind| !(otp && *kind == CredentialKind::XauthPassword))
            .filter(|kind| *kind != CredentialKind::TokenPin)
            .collect()
    }
}
