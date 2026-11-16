//! Formatação da saída do CLI (pt-BR).

use ipsec_client_core::diagnostics::{Check, CheckStatus};
use ipsec_client_core::session::ConnectionInfo;

/// Bytes em unidades binárias legíveis.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Duração legível (`1 h 2 min`, `3 min 4 s`, `5 s`).
pub fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    match (h, m) {
        (0, 0) => format!("{s} s"),
        (0, _) => format!("{m} min {s} s"),
        _ => format!("{h} h {m} min"),
    }
}

/// Símbolo de uma verificação.
pub fn symbol(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "✔",
        CheckStatus::Warn => "⚠",
        CheckStatus::Fail => "✘",
    }
}

/// Linhas de uma verificação.
pub fn check(c: &Check) -> String {
    let mut out = format!("{} {}: {}", symbol(c.status), c.name, c.detail);
    if let Some(hint) = &c.hint {
        out.push_str(&format!("\n    → {hint}"));
    }
    out
}

/// Resumo de uma conexão.
pub fn connection(info: &ConnectionInfo) -> String {
    let profile = info.profile_id.as_deref().unwrap_or("?");
    if !info.is_established() {
        return format!(
            "Engine running (profile {profile}), but no tunnel right now (negotiating or reconnecting)."
        );
    }
    let mut out = format!("Connected: {profile}\n");
    out.push_str(&format!("  Gateway:    {}\n", info.gateway));
    out.push_str(&format!(
        "  Virtual IP: {}\n",
        info.vip.as_deref().unwrap_or("(none)")
    ));
    out.push_str(&format!("  Networks:   {}\n", info.remote_ts.join(", ")));
    if let Some(secs) = info.established_secs {
        out.push_str(&format!("  Up for:     {}\n", duration(secs)));
    }
    out.push_str(&format!(
        "  Traffic:    ↓ {}  ↑ {}",
        bytes(info.bytes_in),
        bytes(info.bytes_out)
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_and_durations() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(duration(5), "5 s");
        assert_eq!(duration(125), "2 min 5 s");
        assert_eq!(duration(3720), "1 h 2 min");
    }
}
