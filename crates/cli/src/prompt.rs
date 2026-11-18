//! Pedido de credenciais no terminal. Senhas e PSK são lidas sem eco
//! (`rpassword`, direto do TTY); o usuário XAuth é lido com eco.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Context, bail};
use ipsec_client_core::profiles::CredentialKind;
use ipsec_client_core::secrets::{Credentials, describe};
use secrecy::SecretString;

/// Lê credenciais da entrada padrão, uma por linha no formato `chave=valor`
/// (chaves: `psk`, `xauth-user`, `xauth-password`, `eap-identity`,
/// `eap-password`, `token-pin`, e `p12-password`, a senha do arquivo de `--client-p12`;
/// `#` comenta). Para automação, como o `docker login --password-stdin`:
/// nunca passe segredos por argumento ou variável de ambiente (aparecem em
/// `ps` e no histórico). Devolve a senha do `.p12`, se veio.
pub fn read_stdin(credentials: &mut Credentials) -> anyhow::Result<Option<SecretString>> {
    parse_credentials(std::io::stdin().lock(), credentials)
}

fn parse_credentials(
    input: impl BufRead,
    credentials: &mut Credentials,
) -> anyhow::Result<Option<SecretString>> {
    let kinds = [
        CredentialKind::Psk,
        CredentialKind::XauthUser,
        CredentialKind::XauthPassword,
        CredentialKind::EapIdentity,
        CredentialKind::EapPassword,
        CredentialKind::TokenPin,
    ];
    let mut p12_password = None;
    for (n, line) in input.lines().enumerate() {
        let line = line?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("input line {}: use key=value", n + 1);
        };
        if key.trim() == "p12-password" {
            p12_password = Some(SecretString::from(value.to_owned()));
            continue;
        }
        let Some(kind) = kinds.iter().find(|k| k.key() == key.trim()) else {
            bail!(
                "input line {}: unknown key (use psk, xauth-user, xauth-password, eap-identity, \
                 eap-password, token-pin or p12-password)",
                n + 1
            );
        };
        credentials.set(*kind, SecretString::from(value.to_owned()));
    }
    Ok(p12_password)
}

/// Senha de um arquivo `.p12`, sem eco.
pub fn ask_p12_password() -> anyhow::Result<SecretString> {
    if !interactive() {
        bail!("the .p12 password is needed and there is no terminal to ask for it");
    }
    let value = rpassword::prompt_password("Password of the .p12 file: ")
        .context("failed to read from the terminal")?;
    Ok(SecretString::from(value))
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
        CredentialKind::EapIdentity => "Username (EAP)",
        CredentialKind::EapPassword => "Password (EAP)",
        CredentialKind::ClientCert | CredentialKind::ClientKey => "Client certificate",
        CredentialKind::TokenKey => "Certificate on the smartcard or token",
        CredentialKind::TokenPin => "PIN of the smartcard or token",
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
    // A certificate is imported from its file, never typed.
    for &kind in kinds.iter().filter(|k| k.is_typed()) {
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
        let p12 = parse_credentials("p12-password=x=y\n".as_bytes(), &mut Credentials::new())
            .expect("p12")
            .expect("password");
        assert_eq!(secrecy::ExposeSecret::expose_secret(&p12), "x=y");
        assert!(parse_credentials("senha".as_bytes(), &mut Credentials::new()).is_err());
        assert!(parse_credentials("token=x".as_bytes(), &mut Credentials::new()).is_err());
    }
}
