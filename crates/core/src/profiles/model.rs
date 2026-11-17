//! Modelo do perfil de conexão (ADR-0008, ADR-0022), serializado em JSON.
//!
//! Um perfil descreve a VPN (gateway, propostas, rotas, DNS) e **nunca** contém
//! credenciais: nem PSK, nem usuário ou senha XAuth (ADR-0007). Assim ele pode
//! ser passado a colegas, e cada pessoa informa as próprias credenciais.
//!
//! Em memória o perfil tem a forma do formato 2 (rodadas de autenticação). O
//! formato 1 é lido e convertido, e o perfil é gravado no formato mais baixo
//! que o expressa (ver `schema.rs`).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Versão mais nova do formato do perfil.
pub const SCHEMA_VERSION: u32 = 2;

/// Perfil de conexão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Identificador (`[a-z0-9-]`): nome do arquivo, da conexão no charon e
    /// chave no chaveiro.
    pub id: String,
    /// Nome exibido.
    pub name: String,
    /// Gateway: nome DNS ou IP.
    pub gateway: String,
    /// Fase 1.
    pub ike: IkeSettings,
    /// Autenticação.
    pub auth: AuthSettings,
    /// Fase 2.
    pub child: ChildSettings,
    /// Rede e DNS.
    #[serde(default)]
    pub network: NetworkSettings,
}

/// Fase 1 (IKE_SA).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IkeSettings {
    /// Versão do IKE: 1 ou 2.
    pub version: u8,
    /// Modo agressivo (só IKEv1).
    #[serde(default)]
    pub aggressive: bool,
    /// Força encapsulamento UDP (NAT-T) mesmo sem NAT.
    #[serde(default)]
    pub encap: bool,
    /// Propostas IKE (ex.: `aes256-sha1-modp1024`), em ordem de preferência.
    pub proposals: Vec<String>,
    /// Intervalo de rekey do IKE_SA, em segundos (ausente = padrão do charon).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rekey_time_secs: Option<u32>,
    /// Intervalo do DPD, em segundos (ausente = sem DPD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpd_delay_secs: Option<u32>,
}

/// Método de uma rodada de autenticação do cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalMethod {
    /// Chave pré-compartilhada (muitas vezes a do grupo).
    Psk,
    /// Usuário e senha XAuth (IKEv1, segunda rodada).
    Xauth,
    /// Usuário e senha por EAP-MSCHAPv2 (IKEv2; o gateway se autentica por
    /// certificado, ADR-0023).
    EapMschapv2,
}

/// Uma rodada de autenticação do cliente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRound {
    /// Método.
    pub method: LocalMethod,
    /// Identidade enviada nesta rodada (ex.: `keyid:nome-do-grupo` na rodada
    /// PSK). Ausente = o endereço do computador.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A senha é um código de uso único: nunca é guardada no chaveiro e é
    /// pedida a cada conexão (só XAuth; ADR-0025).
    #[serde(default, skip_serializing_if = "is_false")]
    pub otp: bool,
}

impl LocalRound {
    /// Rodada sem identidade nem OTP.
    #[must_use]
    pub fn new(method: LocalMethod) -> Self {
        Self {
            method,
            id: None,
            otp: false,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // assinatura exigida pelo serde
fn is_false(value: &bool) -> bool {
    !*value
}

/// Como o gateway prova quem é.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteMethod {
    /// A mesma PSK do cliente.
    Psk,
    /// Certificado, conferido contra a CA do perfil (`ca`) e a identidade
    /// `id` (ADR-0022): nunca "aceitar qualquer certificado".
    Certificate,
}

/// Autenticação do gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAuth {
    /// Método.
    pub method: RemoteMethod,
    /// Identidade que o gateway deve apresentar (ausente = qualquer; com
    /// certificado, obrigatória).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// CAs (PEM) em que o certificado do gateway deve terminar. Dados
    /// públicos, enviados ao charon como dados (nunca como caminho).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ca: Vec<String>,
}

/// Autenticação: as rodadas do cliente, em ordem, e a do gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettings {
    /// Rodadas do cliente (1 ou 2).
    pub local: Vec<LocalRound>,
    /// Gateway.
    pub remote: RemoteAuth,
}

impl AuthSettings {
    /// PSK (com a identidade `psk_id`), opcionalmente seguida de XAuth, e o
    /// gateway autenticado pela mesma PSK.
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

    /// Usuário e senha por EAP-MSCHAPv2, com o gateway autenticado por
    /// certificado da identidade `gateway_id`, emitido por uma das `ca`.
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

    /// Há uma rodada EAP?
    #[must_use]
    pub fn uses_eap(&self) -> bool {
        self.round(LocalMethod::EapMschapv2).is_some()
    }

    fn round(&self, method: LocalMethod) -> Option<&LocalRound> {
        self.local.iter().find(|r| r.method == method)
    }

    /// Identidade da rodada PSK (ex.: id do grupo).
    #[must_use]
    pub fn psk_id(&self) -> Option<&str> {
        self.round(LocalMethod::Psk).and_then(|r| r.id.as_deref())
    }

    /// Identidade esperada do gateway.
    #[must_use]
    pub fn remote_id(&self) -> Option<&str> {
        self.remote.id.as_deref()
    }

    /// Há uma rodada XAuth?
    #[must_use]
    pub fn uses_xauth(&self) -> bool {
        self.round(LocalMethod::Xauth).is_some()
    }

    /// A senha XAuth é de uso único?
    #[must_use]
    pub fn otp(&self) -> bool {
        self.round(LocalMethod::Xauth).is_some_and(|r| r.otp)
    }
}

/// Ação do CHILD_SA após DPD ou fechamento pelo gateway.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildAction {
    /// Não faz nada (o túnel cai).
    #[default]
    None,
    /// Instala uma política de trap: reconecta quando houver tráfego.
    Trap,
    /// Reconecta imediatamente.
    Restart,
}

/// Fase 2 (CHILD_SA).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildSettings {
    /// Nome do child (`[a-z0-9-]`).
    #[serde(default = "default_child_name")]
    pub name: String,
    /// Propostas ESP, em ordem de preferência (incluir o grupo DH ativa PFS).
    pub esp_proposals: Vec<String>,
    /// Intervalo de rekey do CHILD_SA, em segundos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rekey_time_secs: Option<u32>,
    /// Redes remotas acessadas pelo túnel (CIDR ou IP). `0.0.0.0/0` = túnel completo.
    pub remote_ts: Vec<String>,
    /// Redes locais (vazio = o IP virtual recebido).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_ts: Vec<String>,
    /// Ação após o DPD detectar o gateway inacessível.
    #[serde(default)]
    pub dpd_action: ChildAction,
    /// Ação quando o gateway fecha o túnel.
    #[serde(default)]
    pub close_action: ChildAction,
}

fn default_child_name() -> String {
    "rede".to_owned()
}

/// Rede e DNS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NetworkSettings {
    /// Pede um IP virtual ao gateway (mode config).
    pub request_vip: bool,
    /// MTU da interface do túnel.
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

/// DNS da interface do túnel (systemd-resolved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsSettings {
    /// Servidores DNS fixos.
    pub servers: Vec<IpAddr>,
    /// Domínios: `~dominio` só roteia consultas; `dominio` também entra na
    /// busca; `~.` manda todas as consultas para estes servidores.
    pub domains: Vec<String>,
    /// Usa também os servidores enviados pelo gateway.
    pub use_received: bool,
    /// Usa estes servidores para nomes fora dos domínios.
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

/// Credencial que o perfil exige do usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialKind {
    /// PSK (normalmente a do grupo).
    Psk,
    /// Usuário XAuth (ex.: matrícula).
    XauthUser,
    /// Senha XAuth.
    XauthPassword,
    /// Identidade EAP (o usuário; não é segredo, mas é pessoal).
    EapIdentity,
    /// Senha EAP.
    EapPassword,
}

impl CredentialKind {
    /// Nome estável (chave no chaveiro: `<perfil>:<campo>`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Psk => "psk",
            Self::XauthUser => "xauth-user",
            Self::XauthPassword => "xauth-password",
            Self::EapIdentity => "eap-identity",
            Self::EapPassword => "eap-password",
        }
    }

    /// O valor é secreto (nunca exibido)? O usuário XAuth não é segredo, mas é
    /// mascarado na exibição.
    #[must_use]
    pub fn is_secret(self) -> bool {
        !matches!(self, Self::XauthUser | Self::EapIdentity)
    }
}

impl Profile {
    /// Credenciais necessárias para conectar, na ordem das rodadas.
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
            }
        }
        kinds
    }

    /// Credenciais que podem ficar no chaveiro: todas, menos a senha de uso
    /// único (`otp`), que é pedida a cada conexão.
    #[must_use]
    pub fn storable_credentials(&self) -> Vec<CredentialKind> {
        let otp = self.auth.otp();
        self.required_credentials()
            .into_iter()
            .filter(|kind| !(otp && *kind == CredentialKind::XauthPassword))
            .collect()
    }
}
