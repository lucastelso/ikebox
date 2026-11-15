//! `ipsec-client-cli`: a mesma lógica do app desktop, sem GUI.

use clap::Parser;

/// Cliente de VPN IPsec para Linux (VPNs legadas: IKEv1, PSK de grupo + XAuth).
#[derive(Debug, Parser)]
#[command(name = "ipsec-client-cli", version = ipsec_client_core::VERSION)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        super::Cli::command().debug_assert();
    }
}
