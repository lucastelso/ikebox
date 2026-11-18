//! `ikebox-cli`: the same logic as the desktop app, without a GUI.
//!
//! Examples:
//! ```text
//! ikebox-cli doctor
//! ikebox-cli profiles import presets.local/office.json
//! ikebox-cli secrets set office
//! ikebox-cli connect office            # Ctrl+C disconnects
//! ikebox-cli connect office --detach   # stays connected in the background
//! ikebox-cli status
//! ikebox-cli disconnect
//! ikebox-cli component install --bundle target/network-component
//! ```

// The CLI talks to the user through the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod output;
mod prompt;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{CommandFactory, Parser, Subcommand};
use ikebox_core::diagnostics::{self, CheckStatus};
use ikebox_core::profiles::{
    CredentialKind, DnsSettings, LegacyFormat, Profile, ProfileError, ProfileStore, SecurityLevel,
    SwanctlImport, SwanctlImportOptions, detect_legacy, import_legacy, import_swanctl,
    parse_profile, read_profile_file,
};
use ikebox_core::secrets::{
    ClientCertInfo, ClientCertificate, Credentials, KeyringStore, MAX_PKCS12_BYTES, Redactor,
    SecretStore, TokenCertificate, TokenError, default_module, describe, describe_pem,
    list_certificates, mask,
};
use ikebox_core::session::{self, ConnectOptions, SessionEvent, SessionState};
use secrecy::SecretString;
use tokio::sync::mpsc;

/// IPsec VPN client for Linux (legacy VPNs: IKEv1, group PSK + XAuth).
#[derive(Debug, Parser)]
#[command(name = "ikebox-cli", version = ikebox_core::VERSION)]
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
        /// Stores the credentials asked now in the keyring, once connected.
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
        /// Client certificate (.p12/.pfx) for this connection, kept only in memory
        /// (with --remember, in the keyring). Its password is asked, or read from
        /// the `p12-password` line of --credentials-stdin.
        #[arg(long, value_name = "FILE")]
        client_p12: Option<PathBuf>,
        /// PKCS#11 module of the smartcard or token, for a profile whose
        /// certificate is on one: picks the certificate for this connection
        /// only, instead of the one chosen with `secrets token-use`.
        #[arg(long, value_name = "MODULE")]
        token_module: Option<PathBuf>,
        /// Key id (hexadecimal, from `secrets token-list`) when the token has
        /// several certificates.
        #[arg(long, value_name = "HEX", requires = "token_module")]
        token_id: Option<String>,
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
    /// Prints the shell completion script (bash, zsh, fish, elvish, powershell).
    Completions {
        /// Shell.
        shell: clap_complete::Shell,
    },
    /// Prints the manual page (roff), used to build the packages.
    #[command(hide = true)]
    Manpage,
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
    /// Creates the profile from another VPN client's configuration: Cisco
    /// VPN Client (.pcf), vpnc, NetworkManager (.nmconnection), ipsec.conf or a
    /// FortiClient XML backup. Passwords in the file are skipped without being
    /// read.
    ImportLegacy {
        /// The other client's file.
        file: PathBuf,
        /// Its format (default: detected from the name and content).
        #[arg(long, value_enum)]
        format: Option<LegacyArg>,
        /// Connection to import, when the file has several (ipsec.conf).
        #[arg(long)]
        conn: Option<String>,
        /// Profile id (default: derived from the name).
        #[arg(long)]
        id: Option<String>,
        /// Display name (default: the name in the file).
        #[arg(long)]
        name: Option<String>,
        /// VPN DNS server (repeatable); replaces the file's.
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

/// Formats of `profiles import-legacy`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum LegacyArg {
    /// Cisco VPN Client profile.
    Pcf,
    /// vpnc configuration.
    Vpnc,
    /// NetworkManager connection (keyfile).
    Networkmanager,
    /// strongSwan/libreswan ipsec.conf.
    IpsecConf,
    /// FortiClient XML backup.
    Forticlient,
}

impl From<LegacyArg> for LegacyFormat {
    fn from(a: LegacyArg) -> Self {
        match a {
            LegacyArg::Pcf => Self::CiscoPcf,
            LegacyArg::Vpnc => Self::Vpnc,
            LegacyArg::Networkmanager => Self::NetworkManager,
            LegacyArg::IpsecConf => Self::IpsecConf,
            LegacyArg::Forticlient => Self::FortiClient,
        }
    }
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
    /// Imports a client certificate (.p12/.pfx) into the keyring: the private
    /// key and the certificate. The file's password is not stored.
    ImportP12 {
        /// Profile id.
        profile: String,
        /// The .p12/.pfx file.
        file: PathBuf,
        /// Reads the file's password from the first line of standard input.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Lists the certificates on the connected smartcards and tokens (no PIN
    /// needed).
    TokenList {
        /// PKCS#11 module (default: p11-kit's proxy, or OpenSC).
        #[arg(long, value_name = "MODULE")]
        module: Option<PathBuf>,
    },
    /// Chooses the certificate on a smartcard or token for a profile whose
    /// certificate is on one, and remembers the choice in the keyring (the
    /// certificate and where the key is; never the PIN).
    TokenUse {
        /// Profile id.
        profile: String,
        /// PKCS#11 module (default: p11-kit's proxy, or OpenSC).
        #[arg(long, value_name = "MODULE")]
        module: Option<PathBuf>,
        /// Key id (hexadecimal, from `token-list`) when there are several.
        #[arg(long, value_name = "HEX")]
        id: Option<String>,
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
    if let Err(e) = ikebox_core::secrets::disable_core_dumps() {
        eprintln!("warning: could not disable core dumps: {e}");
    }
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    move_legacy_data();
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

/// Up to 0.9 this was IPsec Client: its configuration and keyring entries
/// move to the new names once (ADR-0031).
fn move_legacy_data() {
    if ikebox_core::legacy::move_config() {
        eprintln!(
            "Moved the IPsec Client configuration (this program's name before 1.0) to Ikebox."
        );
    }
    match ikebox_core::legacy::move_keyring() {
        Some(Ok(n)) if n > 0 => {
            eprintln!("Moved {n} stored credential(s) from IPsec Client to Ikebox.")
        }
        Some(Err(e)) => eprintln!(
            "warning: the credentials stored by IPsec Client could not be moved yet ({e}); they will be on the next run"
        ),
        _ => {}
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_env("IKEBOX_LOG").unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!("ikebox_core={level},ikebox_cli={level}"))
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
            client_p12,
            token_module,
            token_id,
        } => {
            let options = ConnectOptions {
                privileged,
                initiate_timeout: Duration::from_secs(timeout.clamp(5, 300)),
            };
            let flags = ConnectFlags {
                detach,
                remember,
                credentials_stdin,
                client_p12,
                token_module,
                token_id,
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
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "ikebox-cli",
                &mut std::io::stdout(),
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Manpage => {
            clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn store() -> anyhow::Result<ProfileStore> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

fn load_profile(id: &str) -> anyhow::Result<Profile> {
    match store()?.load(id) {
        Ok(p) => Ok(p),
        Err(ProfileError::NotFound(_)) => bail!(
            "profile '{id}' not found; see `ikebox-cli profiles list` or import it with \
             `ikebox-cli profiles import <file>`"
        ),
        Err(e) => Err(e.into()),
    }
}

/// System keyring, or `None` (with a warning) if unavailable.
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
    use ikebox_core::component::{self as comp, Bundle, Origin, State};
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

/// Security level of a profile and the reason for each finding. It only
/// warns: weak profiles still connect.
fn security_text(p: &Profile) -> String {
    let report = p.security();
    let mut out = format!("Security: {}\n", report.level.label());
    for finding in &report.findings {
        let mark = if finding.level == SecurityLevel::Weak {
            "✘"
        } else {
            "⚠"
        };
        out.push_str(&format!("  {mark} {}\n", finding.message));
    }
    out
}

/// Shows an imported profile and saves it (or writes it to `output`).
fn save_imported(
    store: &ProfileStore,
    result: &SwanctlImport,
    output: Option<PathBuf>,
    force: bool,
) -> anyhow::Result<()> {
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
            write_private(&path, &ikebox_core::profiles::to_json(p)?)?;
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
            print!("{}", security_text(p));
            println!("  Next step: ikebox-cli secrets set {}", p.id);
        }
    }
    Ok(())
}

fn profiles(cmd: ProfilesCmd) -> anyhow::Result<ExitCode> {
    let store = store()?;
    match cmd {
        ProfilesCmd::List => {
            let list = store.list()?;
            if list.profiles.is_empty() {
                println!(
                    "No profiles in {}. Import one with `ikebox-cli profiles import <file>`.",
                    store.dir().display()
                );
            }
            for p in &list.profiles {
                println!(
                    "{:<24} {:<40} {:<32} {}",
                    p.id,
                    p.name,
                    p.gateway,
                    p.security().level.label()
                );
            }
            for (path, error) in &list.broken {
                eprintln!("⚠ ignored {}: {error}", path.display());
            }
        }
        ProfilesCmd::Show { profile } => {
            let p = load_profile(&profile)?;
            print!("{}", ikebox_core::profiles::to_json(&p)?);
            // stdout stays the profile JSON; the security level goes to stderr.
            eprint!("{}", security_text(&p));
        }
        ProfilesCmd::Import { file, force } => {
            let p = store.import(&file, force)?;
            println!("✔ Profile '{}' ({}) imported.", p.id, p.name);
            print!("{}", security_text(&p));
            println!("  Next step: ikebox-cli secrets set {}", p.id);
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
            println!("  The credentials stay in the keyring: ikebox-cli secrets forget {profile}");
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
            save_imported(&store, &result, output, force)?;
        }
        ProfilesCmd::ImportLegacy {
            file,
            format,
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
            let file_name = file
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let format = match format {
                Some(f) => LegacyFormat::from(f),
                None => detect_legacy(&file_name, &text).ok_or_else(|| {
                    anyhow::anyhow!(
                        "could not tell the format of {}; pass --format (pcf, vpnc, \
                         networkmanager, ipsec-conf, forticlient)",
                        file.display()
                    )
                })?,
            };
            println!("Format: {}", format.label());
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
            let result = import_legacy(&text, format, &options)?;
            save_imported(&store, &result, output, force)?;
        }
        ProfilesCmd::Validate { file } => {
            let json = read_profile_file(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            match parse_profile(&json) {
                Ok(p) => {
                    println!("✔ Profile '{}' is valid.", p.id);
                    print!("{}", security_text(&p));
                }
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

/// Writes a file with mode 0600 (profiles have the organization's data).
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
        SecretsCmd::ImportP12 {
            profile,
            file,
            password_stdin,
        } => {
            let profile = load_profile(&profile)?;
            if !profile.auth.uses_client_certificate() {
                bail!(
                    "profile '{}' does not sign in with a client certificate",
                    profile.id
                );
            }
            let password = if password_stdin {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                SecretString::from(line.trim_end_matches(['\r', '\n']).to_owned())
            } else {
                prompt::ask_p12_password()?
            };
            let cert = read_p12(&file, &password)?;
            let store = KeyringStore::new()?;
            let mut creds = Credentials::load(&store, &profile)?;
            creds.set_client_certificate(&cert);
            creds.save(&store, &profile)?;
            println!("✔ Client certificate stored in the keyring.");
            print_certificate(cert.info());
            println!("  The .p12 file is no longer needed by the app.");
        }
        SecretsCmd::TokenList { module } => {
            let found = token_certificates(module.as_deref())?;
            if found.is_empty() {
                println!("No certificate on the connected smartcards or tokens.");
                return Ok(ExitCode::SUCCESS);
            }
            for cert in &found {
                println!(
                    "• {} (token \"{}\", key id {})",
                    cert.info.as_ref().map_or("?", |i| i.subject.as_str()),
                    cert.token_label,
                    cert.reference.id
                );
                match &cert.info {
                    Ok(info) => print_certificate(info),
                    Err(why) => println!("  cannot be used: {why}"),
                }
            }
        }
        SecretsCmd::TokenUse {
            profile,
            module,
            id,
        } => {
            let profile = load_profile(&profile)?;
            if !profile.auth.uses_token() {
                bail!(
                    "profile '{}' does not use a certificate on a smartcard or token",
                    profile.id
                );
            }
            let cert = choose_token_certificate(module.as_deref(), id.as_deref())?;
            let store = KeyringStore::new()?;
            let mut creds = Credentials::load(&store, &profile)?;
            creds.set(
                CredentialKind::ClientCert,
                SecretString::from(cert.cert_pem.clone()),
            );
            creds.set(
                CredentialKind::TokenKey,
                SecretString::from(cert.reference.to_json()?),
            );
            creds.save(&store, &profile)?;
            println!("✔ Certificate on the token chosen (the PIN is asked at every connection).");
            if let Ok(info) = &cert.info {
                print_certificate(info);
            }
        }
        SecretsCmd::Status { profile } => {
            let profile = load_profile(&profile)?;
            let store = KeyringStore::new()?;
            for kind in profile.required_credentials() {
                let value = store.get(&profile.id, kind)?;
                let shown = match (&value, kind.is_secret()) {
                    (None, _) => "missing".to_owned(),
                    (Some(v), false) if kind == CredentialKind::ClientCert => {
                        match describe_pem(secrecy::ExposeSecret::expose_secret(v)) {
                            Ok(info) => format!(
                                "stored ({} key, valid until {}, {} days)",
                                info.key,
                                info.not_after,
                                info.days_left()
                            ),
                            Err(e) => format!("stored, but unusable: {e}"),
                        }
                    }
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
    client_p12: Option<PathBuf>,
    token_module: Option<PathBuf>,
    token_id: Option<String>,
    verbose: u8,
}

/// The certificates on the tokens of `module` (or the default module).
fn token_certificates(module: Option<&std::path::Path>) -> anyhow::Result<Vec<TokenCertificate>> {
    let module = match module {
        Some(m) => m.to_owned(),
        None => default_module().ok_or(TokenError::NoModule)?,
    };
    Ok(list_certificates(&module)?)
}

/// One certificate of the tokens: the one with key id `id`, or the only one.
fn choose_token_certificate(
    module: Option<&std::path::Path>,
    id: Option<&str>,
) -> anyhow::Result<TokenCertificate> {
    let found = token_certificates(module)?;
    let mut matching: Vec<_> = found
        .into_iter()
        .filter(|c| id.is_none_or(|id| c.reference.id.eq_ignore_ascii_case(id)))
        .collect();
    match matching.len() {
        0 => bail!("no matching certificate on the connected smartcards or tokens"),
        1 => {
            let cert = matching.remove(0);
            if let Err(why) = &cert.info {
                bail!("that certificate cannot be used: {why}");
            }
            Ok(cert)
        }
        n => bail!(
            "{n} certificates on the tokens: choose one with its key id (see `ikebox-cli \
             secrets token-list`)"
        ),
    }
}

/// Reads and checks a .p12 file (the path stays here, in the user's own
/// process; only the key and certificate go on, as data).
fn read_p12(file: &std::path::Path, password: &SecretString) -> anyhow::Result<ClientCertificate> {
    let size = std::fs::metadata(file)
        .with_context(|| format!("could not read {}", file.display()))?
        .len();
    if size > MAX_PKCS12_BYTES as u64 {
        bail!("{} is too large for a certificate file", file.display());
    }
    let data = zeroize::Zeroizing::new(
        std::fs::read(file).with_context(|| format!("could not read {}", file.display()))?,
    );
    ClientCertificate::from_pkcs12(&data, secrecy::ExposeSecret::expose_secret(password))
        .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))
}

fn print_certificate(info: &ClientCertInfo) {
    println!(
        "  {} key, valid until {} ({} days)",
        info.key,
        info.not_after,
        info.days_left()
    );
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
        ref client_p12,
        ref token_module,
        ref token_id,
        verbose,
    } = *flags;
    let profile = load_profile(id)?;
    if profile.auth.uses_token() && detach {
        // The token's key agent lives in this process (ADR-0029).
        bail!(
            "a profile with a smartcard or token cannot use --detach: this command signs for \
             the VPN while it is up"
        );
    }
    if token_module.is_some() && !profile.auth.uses_token() {
        bail!("--token-module is for profiles whose certificate is on a smartcard or token");
    }
    let keyring = if credentials_stdin { None } else { keyring() };
    let mut creds = match &keyring {
        Some(store) => Credentials::load(store, &profile)?,
        None => Credentials::new(),
    };
    let mut p12_password = None;
    if credentials_stdin {
        p12_password = prompt::read_stdin(&mut creds)?;
    }
    if let Some(file) = client_p12 {
        let password = match p12_password {
            Some(password) => password,
            None if credentials_stdin => {
                bail!("--client-p12 needs a p12-password line in the input")
            }
            None => prompt::ask_p12_password()?,
        };
        let cert = read_p12(file, &password)?;
        creds.set_client_certificate(&cert);
    }
    if let Some(module) = token_module {
        let cert = choose_token_certificate(Some(module), token_id.as_deref())?;
        creds.set(
            CredentialKind::ClientCert,
            SecretString::from(cert.cert_pem),
        );
        creds.set(
            CredentialKind::TokenKey,
            SecretString::from(cert.reference.to_json()?),
        );
    }
    let missing = creds.missing(&profile);
    if missing.iter().any(|k| !k.is_typed()) {
        if profile.auth.uses_token() {
            bail!(
                "this profile signs in with a certificate on a smartcard or token: choose it \
                 once with `ikebox-cli secrets token-use {}`, or pass --token-module",
                profile.id
            );
        }
        bail!(
            "this profile signs in with a client certificate: import it once with \
             `ikebox-cli secrets import-p12 {} <file>`, or pass --client-p12 <file>",
            profile.id
        );
    }
    if !missing.is_empty() && credentials_stdin {
        let names: Vec<_> = missing.iter().map(|k| describe(*k)).collect();
        bail!("missing credentials in the input: {}", names.join(", "));
    }
    if !missing.is_empty() {
        if !prompt::interactive() {
            bail!(
                "credentials are missing and there is no terminal to ask for them; run \
                 `ikebox-cli secrets set {}` first",
                profile.id
            );
        }
        println!("Credentials for '{}':", profile.name);
        prompt::ask(&mut creds, &missing, false)?;
    }
    // Stored only after the connection succeeds (audit P2): a mistyped
    // password must not end up in the keyring.
    let store_after = remember && (!missing.is_empty() || client_p12.is_some());

    println!("Connecting '{}' ({})…", profile.name, profile.gateway);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let store = keyring.as_ref().filter(|_| store_after);
    let connecting = session::connect_then_remember(
        &profile,
        &creds,
        options,
        Some(tx),
        store.map(|s| s as &dyn SecretStore),
    );
    tokio::pin!(connecting);
    let (info, stored) = loop {
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
    match stored {
        Some(Ok(())) => println!("✔ Credentials stored in the keyring."),
        Some(Err(e)) => println!("⚠ Connected, but the credentials were not stored: {e}"),
        None if store_after => println!("⚠ No keyring: the credentials were not stored."),
        None => {}
    }

    if detach {
        println!("\nConnected in the background. To disconnect: ikebox-cli disconnect");
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
                    session::stop_token_agent();
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
    // Masks the active profile's identities (group id and XAuth username); with
    // --anonymize, also the gateway, IPs, DNS servers and domains.
    let mut identities: Vec<String> = Vec::new();
    let mut anonymizer = None;
    if let Ok(profile) = store()?.load(&profile_id) {
        identities.extend(profile.auth.psk_id().map(str::to_owned));
        identities.extend(profile.auth.remote_id().map(str::to_owned));
        let mut xauth_user = None;
        if let Some(store) = keyring()
            && let Ok(creds) = Credentials::load(&store, &profile)
        {
            xauth_user = creds
                .xauth_user()
                .or_else(|| creds.eap_identity())
                .map(str::to_owned);
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
        // Anonymized in one go: the same address keeps the same placeholder in the
        // whole log (and across the --follow rounds).
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
