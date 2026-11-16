//! Diagnóstico de falhas de conexão a partir do log do charon.
//!
//! As assinaturas foram capturadas do strongSwan 5.8.2 contra o gateway de
//! teste, provocando cada falha de propósito (ver docs/desenvolvimento.md):
//!
//! | Falha                     | Linha                                              |
//! |---------------------------|----------------------------------------------------|
//! | PSK/id do grupo           | `calculated HASH does not match HASH payload`      |
//! | Usuário/senha XAuth       | `XAuth authentication of '…' (myself) failed`      |
//! | Propostas IKE             | `received NO_PROPOSAL_CHOSEN` antes do IKE_SA      |
//! | Propostas ESP/PFS         | `received NO_PROPOSAL_CHOSEN` depois do IKE_SA     |
//! | Gateway inacessível       | `retransmit … of request` + `not established after`|

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
    /// O gateway não respondeu.
    Unreachable,
    /// Causa não identificada (ver o log).
    Unknown,
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PskOrGroupId => {
                "o gateway recusou a autenticação da fase 1: confira a PSK do grupo e o id do \
                 grupo (auth.local_id; em FortiGate/Cisco costuma ser keyid:<nome-do-grupo>)"
            }
            Self::XauthCredentials => "usuário ou senha XAuth incorretos",
            Self::IkeProposals => {
                "o gateway não aceitou nenhuma proposta da fase 1 (ike.proposals: \
                 criptografia, integridade e grupo DH)"
            }
            Self::EspProposals => {
                "a fase 1 funcionou, mas o gateway não aceitou nenhuma proposta da fase 2 \
                 (child.esp_proposals: algoritmos e grupo DH/PFS)"
            }
            Self::TrafficSelectors => {
                "o gateway recusou as redes pedidas (child.remote_ts/local_ts)"
            }
            Self::Unreachable => {
                "o gateway não respondeu: confira o endereço, a sua rede e se UDP 500/4500 \
                 não está bloqueado (firewall/NAT)"
            }
            Self::Unknown => "a negociação falhou; veja o log para detalhes",
        })
    }
}

/// Deduz a causa provável a partir das linhas de log da tentativa.
#[must_use]
pub fn diagnose<S: AsRef<str>>(lines: &[S]) -> FailureKind {
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
        if line.contains("HASH does not match")
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

    #[test]
    fn diagnoses_real_failures() {
        assert_eq!(diagnose(PSK_ERRADA), FailureKind::PskOrGroupId);
        assert_eq!(diagnose(SENHA_ERRADA), FailureKind::XauthCredentials);
        assert_eq!(diagnose(PROPOSTA_IKE), FailureKind::IkeProposals);
        assert_eq!(diagnose(PROPOSTA_ESP), FailureKind::EspProposals);
        assert_eq!(diagnose(INALCANCAVEL), FailureKind::Unreachable);
    }

    #[test]
    fn other_cases() {
        assert_eq!(
            diagnose(&["received AUTHENTICATION_FAILED error notify"]),
            FailureKind::PskOrGroupId
        );
        assert_eq!(
            diagnose(&["received INVALID_ID_INFORMATION error notify"]),
            FailureKind::TrafficSelectors
        );
        assert_eq!(diagnose::<&str>(&[]), FailureKind::Unknown);
        assert_eq!(diagnose(&["algo inesperado"]), FailureKind::Unknown);
    }
}
