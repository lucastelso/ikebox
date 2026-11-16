//! Pedido de credenciais no terminal. Senhas e PSK são lidas sem eco
//! (`rpassword`, direto do TTY); o usuário XAuth é lido com eco.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Context, bail};
use ipsec_client_core::profiles::CredentialKind;
use ipsec_client_core::secrets::{Credentials, describe};
use secrecy::SecretString;

/// Lê credenciais da entrada padrão, uma por linha no formato `chave=valor`
/// (chaves: `psk`, `xauth-user`, `xauth-password`; `#` comenta). Para
/// automação, como o `docker login --password-stdin`: nunca passe segredos
/// por argumento ou variável de ambiente (aparecem em `ps` e no histórico).
pub fn read_stdin(credentials: &mut Credentials) -> anyhow::Result<()> {
    parse_credentials(std::io::stdin().lock(), credentials)
}

fn parse_credentials(input: impl BufRead, credentials: &mut Credentials) -> anyhow::Result<()> {
    let kinds = [
        CredentialKind::Psk,
        CredentialKind::XauthUser,
        CredentialKind::XauthPassword,
    ];
    for (n, line) in input.lines().enumerate() {
        let line = line?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("input line {}: use key=value", n + 1);
        };
        let Some(kind) = kinds.iter().find(|k| k.key() == key.trim()) else {
            bail!(
                "input line {}: unknown key (use psk, xauth-user or xauth-password)",
                n + 1
            );
        };
        credentials.set(*kind, SecretString::from(value.to_owned()));
    }
    Ok(())
}

/// Há um terminal para perguntar?
pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

fn label(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Psk => "Group PSK",
        CredentialKind::XauthUser => "XAuth username (e.g. employee id)",
        CredentialKind::XauthPassword => "XAuth password",
    }
}

/// Pede as credenciais `kinds`. Com `keep_existing`, Enter vazio mantém o
/// valor atual.
pub fn ask(
    credentials: &mut Credentials,
    kinds: &[CredentialKind],
    keep_existing: bool,
) -> anyhow::Result<()> {
    if !interactive() {
        let names: Vec<_> = kinds.iter().map(|k| describe(*k)).collect();
        bail!(
            "credentials are missing ({}) and there is no terminal to ask for them",
            names.join(", ")
        );
    }
    for &kind in kinds {
        let has_current = credentials.get(kind).is_some();
        let hint = if keep_existing && has_current {
            " [Enter keeps the current one]"
        } else {
            ""
        };
        let prompt = format!("{}{hint}: ", label(kind));
        let value = if kind.is_secret() {
            rpassword::prompt_password(&prompt).context("failed to read from the terminal")?
        } else {
            let mut stderr = std::io::stderr();
            write!(stderr, "{prompt}")?;
            stderr.flush()?;
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line.trim().to_owned()
        };
        if value.is_empty() {
            if keep_existing && has_current {
                continue;
            }
            bail!("{} cannot be empty", describe(kind));
        }
        credentials.set(kind, SecretString::from(value));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stdin_credentials() {
        let input = "# credenciais\npsk=a=b c\nxauth-user=usuario\n\nxauth-password=s3nh@ \n";
        let mut creds = Credentials::new();
        parse_credentials(input.as_bytes(), &mut creds).expect("parse");
        let get = |k| secrecy::ExposeSecret::expose_secret(creds.get(k).expect("valor")).to_owned();
        assert_eq!(get(CredentialKind::Psk), "a=b c");
        assert_eq!(get(CredentialKind::XauthUser), "usuario");
        assert_eq!(get(CredentialKind::XauthPassword), "s3nh@ ");
        assert!(parse_credentials("senha".as_bytes(), &mut Credentials::new()).is_err());
        assert!(parse_credentials("token=x".as_bytes(), &mut Credentials::new()).is_err());
    }
}
