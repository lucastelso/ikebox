//! `ipsec-client-cli`: a mesma lógica do app desktop, sem GUI.
//!
//! Exemplos:
//! ```text
//! ipsec-client-cli doctor
//! ipsec-client-cli profiles import presets.local/trabalho.json
//! ipsec-client-cli secrets set trabalho
//! ipsec-client-cli connect trabalho          # Ctrl+C desconecta
//! ipsec-client-cli connect trabalho --detach # fica conectado em segundo plano
//! ipsec-client-cli status
//! ipsec-client-cli disconnect
//! ipsec-client-cli component install --bundle target/network-component
//! ```

// O CLI fala com o usuário pelo terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod output;
mod prompt;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use ipsec_client_core::diagnostics::{self, CheckStatus};
use ipsec_client_core::profiles::{
    DnsSettings, Profile, ProfileError, ProfileStore, SwanctlImportOptions, import_swanctl,
    parse_profile, read_profile_file,
};
use ipsec_client_core::secrets::{
    Credentials, KeyringStore, Redactor, SecretStore, describe, mask,
};
use ipsec_client_core::session::{self, ConnectOptions, SessionEvent, SessionState};
use tokio::sync::mpsc;

/// IPsec VPN client for Linux (legacy VPNs: IKEv1, group PSK + XAuth).
#[derive(Debug, Parser)]
#[command(name = "ipsec-client-cli", version = ipsec_client_core::VERSION)]
struct Cli {
    /// More detail (-v: charon log also after connecting; -vv: debugging).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks whether this computer is ready (network component, kernel, DNS, ports, keyring).
    Doctor,
    /// Network component (embedded engine): status, installation and removal.
    #[command(subcommand)]
    Component(ComponentCmd),
    /// Manages connection profiles.
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// Manages the credentials stored in the system keyring.
    #[command(subcommand)]
    Secrets(SecretsCmd),
    /// Connects a profile (in the foreground: Ctrl+C disconnects).
    Connect {
        /// Profile id.
        profile: String,
        /// Returns to the terminal after connecting (disconnect with `disconnect`).
        #[arg(long)]
        detach: bool,
        /// Stores the credentials asked now in the keyring.
        #[arg(long)]
        remember: bool,
        /// Fallback privileged container mode (avoid it; see SECURITY.md).
        #[arg(long)]
        privileged: bool,
        /// Maximum negotiation time, in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Reads the credentials from standard input (`key=value` lines), for automation.
        #[arg(long)]
        credentials_stdin: bool,
    },
    /// Disconnects (tells the gateway and stops the engine).
    Disconnect,
    /// Shows the active connection (exit code 3 when disconnected).
    Status,
    /// Shows the engine log (identities masked).
    Logs {
        /// Keeps following.
        #[arg(short, long)]
        follow: bool,
        /// How many trailing lines to show.
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Also replaces gateway, IPs, DNS and domains with placeholders, to
        /// send in a bug report.
        #[arg(long)]
        anonymize: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProfilesCmd {
    /// Lists the profiles.
    List,
    /// Shows a profile (JSON).
    Show {
        /// Profile id.
        profile: String,
    },
    /// Imports a profile file (JSON).
    Import {
        /// File.
        file: PathBuf,
        /// Replaces an existing profile with the same id.
        #[arg(long)]
        force: bool,
    },
    /// Exports a profile (without credentials) to share it.
    Export {
        /// Profile id.
        profile: String,
        /// Destination file.
        file: PathBuf,
    },
    /// Removes a profile (credentials stay; use `secrets forget`).
    Delete {
        /// Profile id.
        profile: String,
    },
    /// Creates a profile from an existing swanctl.conf (without copying secrets).
    ImportSwanctl {
        /// The swanctl.conf (or conf.d file) with the connection.
        file: PathBuf,
        /// Connection to import (default: the first one).
        #[arg(long)]
        conn: Option<String>,
        /// Profile id (default: derived from the connection name).
        #[arg(long)]
        id: Option<String>,
        /// Display name (default: the connection name).
        #[arg(long)]
        name: Option<String>,
        /// VPN DNS server (repeatable; swanctl.conf does not have this information).
        #[arg(long = "dns")]
        dns: Vec<std::net::IpAddr>,
        /// VPN DNS domain (repeatable): `~domain` only routes queries.
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Also uses the DNS servers sent by the gateway.
        #[arg(long)]
        use_received_dns: bool,
        /// Writes the profile to this file instead of saving it to the profile list.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Replaces an existing profile with the same id.
        #[arg(long)]
        force: bool,
    },
    /// Validates a profile file without importing it.
    Validate {
        /// File.
        file: PathBuf,
    },
    /// Shows the profiles directory.
    Path,
}

#[derive(Debug, Subcommand)]
enum ComponentCmd {
    /// Shows what is installed (and whether the given bundle is different).
    Status {
        /// Bundle to compare (directory with SHA256SUMS).
        #[arg(long)]
        bundle: Option<PathBuf>,
    },
    /// Installs or updates from a bundle (asks for the administrator password).
    Install {
        /// Bundle (built by scripts/stage-network-component.sh).
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Removes the component (asks for the administrator password).
    Uninstall,
}

#[derive(Debug, Subcommand)]
enum SecretsCmd {
    /// Asks for the profile credentials and stores them in the keyring.
    Set {
        /// Profile id.
        profile: String,
        /// Reads the credentials from standard input (`key=value` lines), without asking.
        #[arg(long)]
        credentials_stdin: bool,
    },
    /// Shows which credentials are stored (without revealing values).
    Status {
        /// Profile id.
        profile: String,
    },
    /// Deletes the profile credentials from the keyring.
    Forget {
        /// Profile id.
        profile: String,
    },
}

fn main() -> ExitCode {
    if let Err(e) = ipsec_client_core::secrets::disable_core_dumps() {
        eprintln!("warning: could not disable core dumps: {e}");
    }
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: could not start the runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(cli)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("✘ {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("IPSEC_CLIENT_LOG").unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(format!(
                "ipsec_client_core={level},ipsec_client_cli={level}"
            ))
        });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Doctor => doctor().await,
        Command::Component(cmd) => tokio::task::spawn_blocking(move || component(cmd)).await?,
        Command::Profiles(cmd) => profiles(cmd),
        Command::Secrets(cmd) => secrets(cmd),
        Command::Connect {
            profile,
            detach,
            remember,
            privileged,
            timeout,
            credentials_stdin,
        } => {
            let options = ConnectOptions {
                privileged,
                initiate_timeout: Duration::from_secs(timeout.clamp(5, 300)),
            };
            let flags = ConnectFlags {
                detach,
                remember,
                credentials_stdin,
                verbose: cli.verbose,
            };
            connect(&profile, &flags, &options).await
        }
        Command::Disconnect => {
            if session::disconnect().await? {
                println!("✔ Disconnected.");
            } else {
                println!("No active connection.");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Status => match session::status().await? {
            Some(info) => {
                println!("{}", output::connection(&info));
                Ok(ExitCode::SUCCESS)
            }
            None => {
                println!("Disconnected.");
                Ok(ExitCode::from(3))
            }
        },
        Command::Logs {
            follow,
            lines,
            anonymize,
        } => logs(follow, lines, anonymize).await,
    }
}

fn store() -> anyhow::Result<ProfileStore> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

fn load_profile(id: &str) -> anyhow::Result<Profile> {
    match store()?.load(id) {
        Ok(p) => Ok(p),
        Err(ProfileError::NotFound(_)) => bail!(
            "profile '{id}' not found; see `ipsec-client-cli profiles list` or import it with \
             `ipsec-client-cli profiles import <file>`"
        ),
        Err(e) => Err(e.into()),
    }
}

/// Chaveiro do sistema, ou `None` (com aviso) se indisponível.
fn keyring() -> Option<KeyringStore> {
    match KeyringStore::new() {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("⚠ {e}");
            None
        }
    }
}

fn component(cmd: ComponentCmd) -> anyhow::Result<ExitCode> {
    use ipsec_client_core::component::{self as comp, Bundle, Origin, State};
    let open = |dir: &std::path::Path| {
        Bundle::open(dir).with_context(|| format!("bundle in {}", dir.display()))
    };
    let print_log = |log: &[String]| log.iter().for_each(|l| println!("  {l}"));
    match cmd {
        ComponentCmd::Status { bundle } => {
            let bundle = bundle.as_deref().map(open).transpose()?;
            let status = comp::status(bundle.as_ref());
            let state = match status.state {
                State::NotInstalled => "not installed",
                State::Ready => "installed",
                State::Outdated => "installed, different from the bundle (update it)",
                State::Broken => "broken",
            };
            println!("Network component: {state}");
            if let Some(inst) = &status.installed {
                println!(
                    "  version {} · strongSwan {} · origin {}",
                    inst.version.app.as_deref().unwrap_or("?"),
                    inst.version.strongswan.as_deref().unwrap_or("?"),
                    inst.origin.as_str()
                );
            }
            if let Some(problem) = &status.problem {
                println!("  problem: {problem}");
            }
            Ok(if status.state == State::NotInstalled {
                ExitCode::from(3)
            } else {
                ExitCode::SUCCESS
            })
        }
        ComponentCmd::Install { bundle } => {
            let bundle = open(&bundle)?;
            println!("… the system will ask for the administrator password");
            match comp::install(&bundle, Origin::App) {
                Ok(log) => {
                    print_log(&log);
                    println!("✔ Network component installed.");
                    Ok(ExitCode::SUCCESS)
                }
                Err(comp::ComponentError::Failed { log }) => {
                    print_log(&log);
                    bail!("the network component installer failed")
                }
                Err(e) => Err(e.into()),
            }
        }
        ComponentCmd::Uninstall => {
            println!("… the system will ask for the administrator password");
            match comp::uninstall() {
                Ok(log) => {
                    print_log(&log);
                    println!("✔ Network component removed.");
                    Ok(ExitCode::SUCCESS)
                }
                Err(comp::ComponentError::Failed { log }) => {
                    print_log(&log);
                    bail!("removing the network component failed")
                }
                Err(e) => Err(e.into()),
            }
        }
    }
}

async fn doctor() -> anyhow::Result<ExitCode> {
    let checks = diagnostics::doctor().await;
    for c in &checks {
        println!("{}", output::check(c));
    }
    if checks.iter().any(|c| c.status == CheckStatus::Fail) {
        println!("\n✘ There are problems that prevent connecting.");
        Ok(ExitCode::FAILURE)
    } else {
        println!("\n✔ Ready to connect.");
        Ok(ExitCode::SUCCESS)
    }
}

fn profiles(cmd: ProfilesCmd) -> anyhow::Result<ExitCode> {
    let store = store()?;
    match cmd {
        ProfilesCmd::List => {
            let list = store.list()?;
            if list.profiles.is_empty() {
                println!(
                    "No profiles in {}. Import one with `ipsec-client-cli profiles import <file>`.",
                    store.dir().display()
                );
            }
            for p in &list.profiles {
                println!("{:<24} {:<40} {}", p.id, p.name, p.gateway);
            }
            for (path, error) in &list.broken {
                eprintln!("⚠ ignored {}: {error}", path.display());
            }
        }
        ProfilesCmd::Show { profile } => {
            print!(
                "{}",
                ipsec_client_core::profiles::to_json(&load_profile(&profile)?)?
            );
        }
        ProfilesCmd::Import { file, force } => {
            let p = store.import(&file, force)?;
            println!("✔ Profile '{}' ({}) imported.", p.id, p.name);
            println!("  Next step: ipsec-client-cli secrets set {}", p.id);
        }
        ProfilesCmd::Export { profile, file } => {
            store.export(&profile, &file)?;
            println!(
                "✔ Profile exported to {} (without credentials). Send it over a private channel; \
                 the group PSK goes over a different channel.",
                file.display()
            );
        }
        ProfilesCmd::Delete { profile } => {
            store.delete(&profile)?;
            println!("✔ Profile '{profile}' removed.");
            println!(
                "  The credentials stay in the keyring: ipsec-client-cli secrets forget {profile}"
            );
        }
        ProfilesCmd::ImportSwanctl {
            file,
            conn,
            id,
            name,
            dns,
            domains,
            use_received_dns,
            output,
            force,
        } => {
            let text = read_profile_file(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            let options = SwanctlImportOptions {
                connection: conn,
                id,
                name,
                dns: DnsSettings {
                    servers: dns,
                    domains,
                    use_received: use_received_dns,
                    default_route: false,
                },
            };
            let result = import_swanctl(&text, &options)?;
            let p = &result.profile;
            for warning in &result.warnings {
                println!("⚠ {warning}");
            }
            println!("Profile created (no secrets):");
            println!("  id:        {}", p.id);
            println!("  name:      {}", p.name);
            println!("  gateway:   {}", p.gateway);
            println!(
                "  IKE:       v{}{}",
                p.ike.version,
                if p.ike.aggressive { " aggressive" } else { "" }
            );
            println!("  networks:  {}", p.child.remote_ts.join(", "));
            if p.network.dns.servers.is_empty() && p.network.dns.domains.is_empty() {
                println!("  DNS:       (none; use --dns/--domain if the VPN has internal DNS)");
            }
            match output {
                Some(path) => {
                    write_private(&path, &ipsec_client_core::profiles::to_json(p)?)?;
                    println!(
                        "✔ Written to {} (0600). Import it with `profiles import`.",
                        path.display()
                    );
                }
                None => {
                    if !force && store.load(&p.id).is_ok() {
                        bail!(
                            "a profile '{}' already exists; use --force to replace it",
                            p.id
                        );
                    }
                    store.save(p)?;
                    println!("✔ Profile '{}' saved.", p.id);
                    println!("  Next step: ipsec-client-cli secrets set {}", p.id);
                }
            }
        }
        ProfilesCmd::Validate { file } => {
            let json = read_profile_file(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            match parse_profile(&json) {
                Ok(p) => println!("✔ Profile '{}' is valid.", p.id),
                Err(ProfileError::Invalid(issues)) => {
                    for issue in issues {
                        println!("✘ {issue}");
                    }
                    return Ok(ExitCode::FAILURE);
                }
                Err(e) => return Err(e.into()),
            }
        }
        ProfilesCmd::Path => println!("{}", store.dir().display()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Grava um arquivo com permissão 0600 (perfis têm dados da organização).
fn write_private(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("could not write {}", path.display()))?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

fn secrets(cmd: SecretsCmd) -> anyhow::Result<ExitCode> {
    match cmd {
        SecretsCmd::Set {
            profile,
            credentials_stdin,
        } => {
            let profile = load_profile(&profile)?;
            let store = KeyringStore::new()?;
            let mut creds = Credentials::load(&store, &profile)?;
            if credentials_stdin {
                prompt::read_stdin(&mut creds)?;
            } else {
                println!(
                    "Credentials for '{}' (stored in the system keyring):",
                    profile.name
                );
                prompt::ask(&mut creds, &profile.required_credentials(), true)?;
            }
            creds.save(&store, &profile)?;
            println!("✔ Credentials stored.");
        }
        SecretsCmd::Status { profile } => {
            let profile = load_profile(&profile)?;
            let store = KeyringStore::new()?;
            for kind in profile.required_credentials() {
                let value = store.get(&profile.id, kind)?;
                let shown = match (&value, kind.is_secret()) {
                    (None, _) => "missing".to_owned(),
                    (Some(_), true) => "stored".to_owned(),
                    (Some(v), false) => {
                        format!("stored ({})", mask(secrecy::ExposeSecret::expose_secret(v)))
                    }
                };
                println!("{:<22} {shown}", describe(kind));
            }
        }
        SecretsCmd::Forget { profile } => {
            let profile = load_profile(&profile)?;
            Credentials::forget(&KeyringStore::new()?, &profile)?;
            println!(
                "✔ Credentials for '{}' deleted from the keyring.",
                profile.id
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_event(event: &SessionEvent, show_logs: bool) {
    match event {
        SessionEvent::State(SessionState::Preparing(step)) => println!("… {step}"),
        SessionEvent::State(SessionState::Negotiating) => {
            println!("… negotiating with the gateway")
        }
        SessionEvent::State(SessionState::Connected(_)) => {}
        SessionEvent::Build(line) if show_logs => println!("  [build] {line}"),
        SessionEvent::Log(line) if show_logs => println!("  {line}"),
        SessionEvent::TunnelDown => {
            println!("⚠ The tunnel dropped; charon will try to reconnect as the profile says.")
        }
        SessionEvent::TunnelUp => println!("✔ Tunnel (re)established."),
        _ => {}
    }
}

struct ConnectFlags {
    detach: bool,
    remember: bool,
    credentials_stdin: bool,
    verbose: u8,
}

async fn connect(
    id: &str,
    flags: &ConnectFlags,
    options: &ConnectOptions,
) -> anyhow::Result<ExitCode> {
    let ConnectFlags {
        detach,
        remember,
        credentials_stdin,
        verbose,
    } = *flags;
    let profile = load_profile(id)?;
    let keyring = if credentials_stdin { None } else { keyring() };
    let mut creds = match &keyring {
        Some(store) => Credentials::load(store, &profile)?,
        None => Credentials::new(),
    };
    if credentials_stdin {
        prompt::read_stdin(&mut creds)?;
    }
    let missing = creds.missing(&profile);
    if !missing.is_empty() && credentials_stdin {
        let names: Vec<_> = missing.iter().map(|k| describe(*k)).collect();
        bail!("missing credentials in the input: {}", names.join(", "));
    }
    if !missing.is_empty() {
        if !prompt::interactive() {
            bail!(
                "credentials are missing and there is no terminal to ask for them; run \
                 `ipsec-client-cli secrets set {}` first",
                profile.id
            );
        }
        println!("Credentials for '{}':", profile.name);
        prompt::ask(&mut creds, &missing, false)?;
        if remember {
            match &keyring {
                Some(store) => {
                    creds.save(store, &profile)?;
                    println!("✔ Credentials stored in the keyring.");
                }
                None => println!("⚠ No keyring: the credentials were not stored."),
            }
        }
    }

    println!("Connecting '{}' ({})…", profile.name, profile.gateway);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let connecting = session::connect(&profile, &creds, options, Some(tx));
    tokio::pin!(connecting);
    let info = loop {
        tokio::select! {
            result = &mut connecting => break result?,
            Some(event) = rx.recv() => print_event(&event, true),
            _ = tokio::signal::ctrl_c() => {
                println!("\nCancelling…");
                let _ = session::disconnect().await;
                bail!("connection cancelled");
            }
        }
    };
    while let Ok(event) = rx.try_recv() {
        print_event(&event, verbose > 0);
    }
    println!("✔ {}", output::connection(&info));

    if detach {
        println!("\nConnected in the background. To disconnect: ipsec-client-cli disconnect");
        return Ok(ExitCode::SUCCESS);
    }
    println!("\nPress Ctrl+C to disconnect.");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\nDisconnecting…");
                session::disconnect().await?;
                println!("✔ Disconnected.");
                return Ok(ExitCode::SUCCESS);
            }
            event = rx.recv() => match event {
                Some(SessionEvent::EngineGone) | None => {
                    println!("✘ The engine stopped (disconnected by another process or an error).");
                    return Ok(ExitCode::FAILURE);
                }
                Some(event) => print_event(&event, verbose > 0),
            }
        }
    }
}

async fn logs(follow: bool, lines: usize, anonymize: bool) -> anyhow::Result<ExitCode> {
    let Some(profile_id) = session::active_profile().await? else {
        println!("No active connection.");
        return Ok(ExitCode::from(3));
    };
    // Mascara as identidades do perfil ativo (id do grupo e usuário XAuth);
    // com --anonymize, também gateway, IPs, DNS e domínios.
    let mut identities: Vec<String> = Vec::new();
    let mut anonymizer = None;
    if let Ok(profile) = store()?.load(&profile_id) {
        identities.extend(profile.auth.local_id.clone());
        let mut xauth_user = None;
        if let Some(store) = keyring()
            && let Ok(creds) = Credentials::load(&store, &profile)
        {
            xauth_user = creds.xauth_user().map(str::to_owned);
            identities.extend(xauth_user.clone());
        }
        if anonymize {
            anonymizer = Some(session::anonymizer_for(&profile, xauth_user.as_deref()));
        }
    }
    if anonymize && anonymizer.is_none() {
        anyhow::bail!("could not read the active profile ({profile_id}) to anonymize the log");
    }
    let redactor = Redactor::new(identities.iter().map(String::as_str));
    let mut printed = 0usize;
    loop {
        let all = session::engine_logs(lines.max(printed + 200)).await?;
        let start = if printed == 0 {
            all.len().saturating_sub(lines)
        } else {
            printed.min(all.len())
        };
        // Anonimizado de uma vez: o mesmo endereço mantém o mesmo marcador
        // em todo o log (e entre as rodadas do --follow).
        let rendered: Vec<String> = match &anonymizer {
            Some(a) => a
                .anonymize(&all.join("\n"))
                .split('\n')
                .map(str::to_owned)
                .collect(),
            None => all.iter().map(|line| redactor.redact(line)).collect(),
        };
        for line in rendered.iter().skip(start) {
            println!("{line}");
        }
        printed = all.len();
        if !follow || session::active_profile().await?.is_none() {
            return Ok(ExitCode::SUCCESS);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        super::Cli::command().debug_assert();
    }
}
