//! Connection failure diagnosis from the charon log.
//!
//! The signatures were captured from strongSwan 5.8.2 against the test
//! gateway, causing each failure on purpose (see docs/development-log.md):
//!
//! | Failure                   | Line                                               |
//! |---------------------------|----------------------------------------------------|
//! | PSK/group id              | `calculated HASH does not match HASH payload`      |
//! | XAuth username/password   | `XAuth authentication of '…' (myself) failed`      |
//! | IKE proposals             | `received NO_PROPOSAL_CHOSEN` before the IKE_SA    |
//! | ESP/PFS proposals         | `received NO_PROPOSAL_CHOSEN` after the IKE_SA     |
//! | Gateway unreachable       | `retransmit … of request` + `not established after`|
//!
//! IKEv2 lines from strongSwan 6.1.0 against the mode test gateways
//! (`npm run test:modes`, phase 6):
//!
//! | Failure                   | Line                                               |
//! |---------------------------|----------------------------------------------------|
//! | PSK or gateway identity   | `received AUTHENTICATION_FAILED notify error`      |
//! | IKE proposals             | `received NO_PROPOSAL_CHOSEN notify error`         |
//! | ESP proposals             | `received NO_PROPOSAL_CHOSEN notify, no CHILD_SA built` (after the IKE_SA) |
//! | Networks                  | `received TS_UNACCEPTABLE notify, no CHILD_SA built` |
//! | Virtual IP required       | `received FAILED_CP_REQUIRED notify, no CHILD_SA built` |
//!
//! IKEv1 main mode with a wrong PSK: the gateway's answers are encrypted with
//! a key derived from its PSK, so they do not decrypt (`invalid HASH_V1
//! payload length, decryption failed?`, `could not decrypt payloads`) and the
//! client retransmits until it gives up. Without this rule it looked like an
//! unreachable gateway.
//!
//! IKEv2 + EAP-MSCHAPv2 (6.1.0, phase 8):
//!
//! | Failure                    | Line                                              |
//! |----------------------------|---------------------------------------------------|
//! | Username/password          | `EAP-MS-CHAPv2 failed with error ERROR_AUTHENTICATION_FAILURE` |
//! | Gateway certificate        | `no issuer certificate found for` / `no trusted RSA public key found for` |
//! | Gateway name (`remote.id`) | `received AUTHENTICATION_FAILED notify error`, before any EAP (a strongSwan gateway refuses an IDr that is not its own) |
//!
//! A gateway that rejects the identity the profile expects from it
//! (`auth.remote_id`) answers with the same AUTHENTICATION_FAILED as a wrong
//! PSK: the message names both.

use std::fmt;

/// Causa provável de uma falha de conexão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// PSK errada ou id do grupo (identidade local) errado.
    PskOrGroupId,
    /// Usuário ou senha XAuth errados.
    XauthCredentials,
    /// Nenhuma proposta IKE (fase 1) aceita pelo gateway.
    IkeProposals,
    /// Nenhuma proposta ESP (fase 2) aceita: algoritmos ou grupo DH (PFS).
    EspProposals,
    /// Redes (traffic selectors) recusadas pelo gateway.
    TrafficSelectors,
    /// O gateway exige que o cliente peça um IP virtual (IKEv2).
    VirtualIpRequired,
    /// Usuário ou senha EAP errados.
    EapCredentials,
    /// O certificado do gateway não pôde ser conferido (CA, nome ou chave).
    GatewayCertificate,
    /// Num perfil EAP, o gateway recusou antes da senha: o nome pedido a ele
    /// (`auth.remote.id`) não é o dele.
    GatewayName,
    /// O gateway não respondeu.
    Unreachable,
    /// Causa não identificada (ver o log).
    Unknown,
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PskOrGroupId => crate::tr!(
                "the gateway rejected phase 1 authentication: check the PSK and the \
                 identities (auth.local_id, the group id: on FortiGate/Cisco it is usually \
                 keyid:<group-name>; and auth.remote_id, if the profile has one)",
                "o gateway recusou a autenticação da fase 1: confira a PSK e as identidades \
                 (auth.local_id, o id do grupo: em FortiGate/Cisco costuma ser \
                 keyid:<nome-do-grupo>; e auth.remote_id, se o perfil tiver)"
            ),
            Self::XauthCredentials => crate::tr!(
                "wrong XAuth username or password",
                "usuário ou senha XAuth incorretos"
            ),
            Self::IkeProposals => crate::tr!(
                "the gateway accepted none of the phase 1 proposals (ike.proposals: \
                 encryption, integrity and DH group)",
                "o gateway não aceitou nenhuma proposta da fase 1 (ike.proposals: \
                 criptografia, integridade e grupo DH)"
            ),
            Self::EspProposals => crate::tr!(
                "phase 1 worked, but the gateway accepted none of the phase 2 proposals \
                 (child.esp_proposals: algorithms and DH/PFS group)",
                "a fase 1 funcionou, mas o gateway não aceitou nenhuma proposta da fase 2 \
                 (child.esp_proposals: algoritmos e grupo DH/PFS)"
            ),
            Self::TrafficSelectors => crate::tr!(
                "the gateway rejected the requested networks (child.remote_ts/local_ts)",
                "o gateway recusou as redes pedidas (child.remote_ts/local_ts)"
            ),
            Self::VirtualIpRequired => crate::tr!(
                "phase 1 worked, but the gateway requires the client to ask for a virtual \
                 IP: turn on network.request_vip",
                "a fase 1 funcionou, mas o gateway exige que o cliente peça um IP virtual: \
                 ligue network.request_vip"
            ),
            Self::EapCredentials => crate::tr!(
                "wrong username or password (EAP)",
                "usuário ou senha incorretos (EAP)"
            ),
            Self::GatewayCertificate => crate::tr!(
                "the gateway's certificate could not be verified: it must be issued by the CA \
                 in the profile and carry the gateway's name (auth.remote.ca, auth.remote.id), \
                 with an RSA or Ed25519 key",
                "o certificado do gateway não pôde ser conferido: ele deve ser emitido pela CA \
                 do perfil e trazer o nome do gateway (auth.remote.ca, auth.remote.id), com \
                 chave RSA ou Ed25519"
            ),
            Self::GatewayName => crate::tr!(
                "the gateway refused the connection before the password: check the gateway's \
                 name (auth.remote.id), which must be the one in its certificate",
                "o gateway recusou a conexão antes da senha: confira o nome do gateway \
                 (auth.remote.id), que deve ser o do certificado dele"
            ),
            Self::Unreachable => crate::tr!(
                "the gateway did not answer: check the address, your network and whether \
                 UDP 500/4500 is blocked (firewall/NAT)",
                "o gateway não respondeu: confira o endereço, a sua rede e se UDP 500/4500 \
                 não está bloqueado (firewall/NAT)"
            ),
            Self::Unknown => crate::tr!(
                "the negotiation failed; see the log for details",
                "a negociação falhou; veja o log para detalhes"
            ),
        })
    }
}

/// Deduz a causa provável a partir das linhas de log da tentativa. `eap`:
/// o perfil usa EAP (sem PSK), o que muda o sentido de um
/// AUTHENTICATION_FAILED antes do IKE_SA.
#[must_use]
pub fn diagnose<S: AsRef<str>>(lines: &[S], eap: bool) -> FailureKind {
    let mut ike_established = false;
    let mut retransmits = false;
    let mut timed_out = false;
    for line in lines.iter().map(AsRef::as_ref) {
        if line.contains("IKE_SA") && line.contains("established between") {
            ike_established = true;
        }
        if line.contains("XAuth authentication of") && line.contains("failed") {
            return FailureKind::XauthCredentials;
        }
        if line.contains("EAP-MS-CHAPv2 failed")
            || (line.contains("EAP method") && line.contains("failed"))
        {
            return FailureKind::EapCredentials;
        }
        if line.contains("no issuer certificate found")
            || (line.contains("no trusted") && line.contains("public key found"))
            || line.contains("constraint check failed")
        {
            return FailureKind::GatewayCertificate;
        }
        if eap && !ike_established && line.contains("AUTHENTICATION_FAILED") {
            return FailureKind::GatewayName;
        }
        if line.contains("HASH does not match")
            || line.contains("decryption failed")
            || line.contains("could not decrypt payloads")
            || (!ike_established && line.contains("AUTHENTICATION_FAILED"))
        {
            return FailureKind::PskOrGroupId;
        }
        if line.contains("NO_PROPOSAL_CHOSEN") {
            return if ike_established {
                FailureKind::EspProposals
            } else {
                FailureKind::IkeProposals
            };
        }
        if line.contains("FAILED_CP_REQUIRED") {
            return FailureKind::VirtualIpRequired;
        }
        if line.contains("INVALID_ID_INFORMATION") || line.contains("TS_UNACCEPTABLE") {
            return FailureKind::TrafficSelectors;
        }
        if line.contains("retransmit") && line.contains("of request") {
            retransmits = true;
        }
        if line.contains("not established after") || line.contains("giving up after") {
            timed_out = true;
        }
    }
    if !ike_established && (retransmits || timed_out) {
        FailureKind::Unreachable
    } else {
        FailureKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Linhas reais do charon 5.8.2 (gateway de teste, identidades de teste).
    const PSK_ERRADA: &[&str] = &[
        "initiating Aggressive Mode IKE_SA teste[1] to 172.19.0.2",
        "selected proposal: IKE:AES_CBC_256/HMAC_SHA1_96/PRF_HMAC_SHA1/MODP_1024",
        "calculated HASH does not match HASH payload",
        "establishing CHILD_SA 'rede-teste' failed",
    ];
    const SENHA_ERRADA: &[&str] = &[
        "initiating Aggressive Mode IKE_SA teste[2] to 172.19.0.2",
        "remote host is behind NAT",
        "XAuth authentication of 'usuario.teste' (myself) failed",
        "establishing CHILD_SA 'rede-teste' failed",
    ];
    const PROPOSTA_IKE: &[&str] = &[
        "initiating Aggressive Mode IKE_SA teste[3] to 172.19.0.2",
        "received NO_PROPOSAL_CHOSEN error notify",
        "establishing CHILD_SA 'rede-teste' failed",
    ];
    const PROPOSTA_ESP: &[&str] = &[
        "XAuth authentication of 'usuario.teste' (myself) successful",
        "IKE_SA teste[4] established between 172.19.0.3[grupo-teste]...172.19.0.2[gw.ipsec-client.test]",
        "installing new virtual IP 198.51.100.10",
        "received NO_PROPOSAL_CHOSEN error notify",
        "establishing CHILD_SA 'rede-teste' failed",
    ];
    const INALCANCAVEL: &[&str] = &[
        "initiating Aggressive Mode IKE_SA teste[5] to 192.0.2.250",
        "retransmit 1 of request message ID 0, seq 1",
        "retransmit 2 of request message ID 0, seq 1",
        "establishing CHILD_SA 'rede-teste' not established after 25000ms",
    ];

    // Linhas reais do charon 6.1.0 em IKEv2 (gateways de teste dos modos).
    const V2_PSK_ERRADA: &[&str] = &[
        "initiating IKE_SA v2-bad-psk[1] to 172.19.0.5",
        "selected proposal: IKE:AES_GCM_16_256/PRF_HMAC_SHA2_256/CURVE_25519",
        "authentication of '172.19.0.6' (myself) with pre-shared key",
        "establishing CHILD_SA rede{1}",
        "received AUTHENTICATION_FAILED notify error",
    ];
    const V2_PROPOSTA_IKE: &[&str] = &[
        "initiating IKE_SA v2-bad-ike[1] to 172.19.0.5",
        "received NO_PROPOSAL_CHOSEN notify error",
    ];
    const V2_PROPOSTA_ESP: &[&str] = &[
        "authentication of 'gw.ipsec-client.test' with pre-shared key successful",
        "installing new virtual IP 198.51.100.10",
        "IKE_SA v2-bad-esp[1] established between 172.19.0.6[172.19.0.6]...172.19.0.5[gw.ipsec-client.test]",
        "received NO_PROPOSAL_CHOSEN notify, no CHILD_SA built",
        "failed to establish CHILD_SA, keeping IKE_SA",
    ];
    const V2_SEM_VIP: &[&str] = &[
        "IKE_SA v2-psk-novip[1] established between 172.19.0.6[172.19.0.6]...172.19.0.5[gw***st]",
        "received FAILED_CP_REQUIRED notify, no CHILD_SA built",
        "failed to establish CHILD_SA, keeping IKE_SA",
    ];
    const V2_REDES: &[&str] = &[
        "IKE_SA v2-bad-ts[1] established between 172.19.0.6[172.19.0.6]...172.19.0.5[gw.ipsec-client.test]",
        "received TS_UNACCEPTABLE notify, no CHILD_SA built",
    ];

    // IKEv1 modo principal com PSK errada (6.1.0): as respostas não decifram.
    const V1_PRINCIPAL_PSK_ERRADA: &[&str] = &[
        "initiating Main Mode IKE_SA v1-main-bad-psk[1] to 172.19.0.3",
        "generating ID_PROT request 0 [ ID HASH ]",
        "invalid HASH_V1 payload length, decryption failed?",
        "message parsing failed",
        "could not decrypt payloads",
        "sending retransmit 1 of request message ID 0, seq 3",
        "giving up after 5 retransmits",
    ];

    // IKEv2 + EAP-MSCHAPv2, 6.1.0 (identidades mascaradas como no log).
    const EAP_SENHA_ERRADA: &[&str] = &[
        "authentication of 'gw***st' with RSA_EMSA_PKCS1_SHA2_256 successful",
        "server requested EAP_IDENTITY (id 0x00), sending 'us***te'",
        "server requested EAP_MSCHAPV2 authentication (id 0xC8)",
        "EAP-MS-CHAPv2 failed with error ERROR_AUTHENTICATION_FAILURE: '(***'",
        "received AUTHENTICATION_FAILED notify error",
    ];
    const EAP_OUTRA_CA: &[&str] = &[
        "received end entity cert \"O=Example, CN=gw***st\"",
        "  using certificate \"O=Example, CN=gw***st\"",
        "no issuer certificate found for \"O=Example, CN=gw***st\"",
        "  issuer is \"O=***CA\"",
        "no trusted RSA public key found for 'gw***st'",
    ];
    const EAP_NOME_ERRADO: &[&str] = &[
        "sending cert request for \"O=***CA\"",
        "no IDi configured, default to IP address 172.19.0.7",
        "establishing CHILD_SA rede{1}",
        "received AUTHENTICATION_FAILED notify error",
    ];

    #[test]
    fn diagnoses_real_eap_failures() {
        assert_eq!(
            diagnose(EAP_SENHA_ERRADA, true),
            FailureKind::EapCredentials
        );
        assert_eq!(
            diagnose(EAP_OUTRA_CA, true),
            FailureKind::GatewayCertificate
        );
        assert_eq!(diagnose(EAP_NOME_ERRADO, true), FailureKind::GatewayName);
        // The same line in a PSK profile is still about the PSK.
        assert_eq!(diagnose(EAP_NOME_ERRADO, false), FailureKind::PskOrGroupId);
    }

    #[test]
    fn diagnoses_real_failures_of_the_mode_tests() {
        assert_eq!(
            diagnose(V1_PRINCIPAL_PSK_ERRADA, false),
            FailureKind::PskOrGroupId
        );
        assert_eq!(diagnose(V2_PSK_ERRADA, false), FailureKind::PskOrGroupId);
        assert_eq!(diagnose(V2_PROPOSTA_IKE, false), FailureKind::IkeProposals);
        assert_eq!(diagnose(V2_PROPOSTA_ESP, false), FailureKind::EspProposals);
        assert_eq!(diagnose(V2_SEM_VIP, false), FailureKind::VirtualIpRequired);
        assert_eq!(diagnose(V2_REDES, false), FailureKind::TrafficSelectors);
    }

    #[test]
    fn diagnoses_real_failures() {
        assert_eq!(diagnose(PSK_ERRADA, false), FailureKind::PskOrGroupId);
        assert_eq!(diagnose(SENHA_ERRADA, false), FailureKind::XauthCredentials);
        assert_eq!(diagnose(PROPOSTA_IKE, false), FailureKind::IkeProposals);
        assert_eq!(diagnose(PROPOSTA_ESP, false), FailureKind::EspProposals);
        assert_eq!(diagnose(INALCANCAVEL, false), FailureKind::Unreachable);
    }

    #[test]
    fn other_cases() {
        assert_eq!(
            diagnose(&["received AUTHENTICATION_FAILED error notify"], false),
            FailureKind::PskOrGroupId
        );
        assert_eq!(
            diagnose(&["received INVALID_ID_INFORMATION error notify"], false),
            FailureKind::TrafficSelectors
        );
        assert_eq!(diagnose::<&str>(&[], false), FailureKind::Unknown);
        assert_eq!(diagnose(&["algo inesperado"], false), FailureKind::Unknown);
    }
}
