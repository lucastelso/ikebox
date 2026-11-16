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
    parse_profile,
};
use ipsec_client_core::secrets::{
    Credentials, KeyringStore, Redactor, SecretStore, describe, mask,
};
use ipsec_client_core::session::{self, ConnectOptions, SessionEvent, SessionState};
use tokio::sync::mpsc;

/// Cliente de VPN IPsec para Linux (VPNs legadas: IKEv1, PSK de grupo + XAuth).
#[derive(Debug, Parser)]
#[command(name = "ipsec-client-cli", version = ipsec_client_core::VERSION)]
struct Cli {
    /// Mais detalhes (-v: log do charon também depois de conectar; -vv: depuração).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verifica se o ambiente está pronto (componente de rede, kernel, DNS, portas, chaveiro).
    Doctor,
    /// Componente de rede (engine embutido): estado, instalação e remoção.
    #[command(subcommand)]
    Component(ComponentCmd),
    /// Gerencia perfis de conexão.
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// Gerencia as credenciais guardadas no chaveiro do sistema.
    #[command(subcommand)]
    Secrets(SecretsCmd),
    /// Conecta um perfil (em primeiro plano: Ctrl+C desconecta).
    Connect {
        /// Id do perfil.
        profile: String,
        /// Volta ao terminal depois de conectar (desconecte com `disconnect`).
        #[arg(long)]
        detach: bool,
        /// Guarda no chaveiro as credenciais pedidas agora.
        #[arg(long)]
        remember: bool,
        /// Modo privilegiado de reserva do container (evite; ver SECURITY.md).
        #[arg(long)]
        privileged: bool,
        /// Tempo máximo da negociação, em segundos.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Lê as credenciais da entrada padrão (linhas `chave=valor`), para automação.
        #[arg(long)]
        credentials_stdin: bool,
    },
    /// Desconecta (avisa o gateway e derruba o engine).
    Disconnect,
    /// Mostra a conexão ativa (código de saída 3 se desconectado).
    Status,
    /// Mostra o log do engine (identidades mascaradas).
    Logs {
        /// Continua acompanhando.
        #[arg(short, long)]
        follow: bool,
        /// Quantas linhas finais mostrar.
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(Debug, Subcommand)]
enum ProfilesCmd {
    /// Lista os perfis.
    List,
    /// Mostra um perfil (JSON).
    Show {
        /// Id do perfil.
        profile: String,
    },
    /// Importa um arquivo de perfil (JSON).
    Import {
        /// Arquivo.
        file: PathBuf,
        /// Substitui um perfil existente com o mesmo id.
        #[arg(long)]
        force: bool,
    },
    /// Exporta um perfil (sem credenciais) para compartilhar.
    Export {
        /// Id do perfil.
        profile: String,
        /// Arquivo de destino.
        file: PathBuf,
    },
    /// Remove um perfil (as credenciais ficam; use `secrets forget`).
    Delete {
        /// Id do perfil.
        profile: String,
    },
    /// Cria um perfil a partir de um swanctl.conf existente (sem copiar segredos).
    ImportSwanctl {
        /// O swanctl.conf (ou arquivo de conf.d) com a conexão.
        file: PathBuf,
        /// Conexão a importar (padrão: a primeira).
        #[arg(long)]
        conn: Option<String>,
        /// Id do perfil (padrão: derivado do nome da conexão).
        #[arg(long)]
        id: Option<String>,
        /// Nome exibido (padrão: o nome da conexão).
        #[arg(long)]
        name: Option<String>,
        /// Servidor DNS da VPN (repetível; o swanctl.conf não tem essa informação).
        #[arg(long = "dns")]
        dns: Vec<std::net::IpAddr>,
        /// Domínio DNS da VPN (repetível): `~dominio` só roteia consultas.
        #[arg(long = "domain")]
        domains: Vec<String>,
        /// Usa também os DNS enviados pelo gateway.
        #[arg(long)]
        use_received_dns: bool,
        /// Grava o perfil neste arquivo em vez de salvá-lo na lista de perfis.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Substitui um perfil existente com o mesmo id.
        #[arg(long)]
        force: bool,
    },
    /// Valida um arquivo de perfil sem importar.
    Validate {
        /// Arquivo.
        file: PathBuf,
    },
    /// Mostra o diretório dos perfis.
    Path,
}

#[derive(Debug, Subcommand)]
enum ComponentCmd {
    /// Mostra o que está instalado (e se o pacote indicado é diferente).
    Status {
        /// Pacote para comparar (diretório com SHA256SUMS).
        #[arg(long)]
        bundle: Option<PathBuf>,
    },
    /// Instala ou atualiza a partir de um pacote (pede a senha de administrador).
    Install {
        /// Pacote (gerado por scripts/stage-network-component.sh).
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Remove o componente (pede a senha de administrador).
    Uninstall,
}

#[derive(Debug, Subcommand)]
enum SecretsCmd {
    /// Pede e guarda no chaveiro as credenciais do perfil.
    Set {
        /// Id do perfil.
        profile: String,
        /// Lê as credenciais da entrada padrão (linhas `chave=valor`), sem perguntar.
        #[arg(long)]
        credentials_stdin: bool,
    },
    /// Mostra quais credenciais estão guardadas (sem revelar valores).
    Status {
        /// Id do perfil.
        profile: String,
    },
    /// Apaga do chaveiro as credenciais do perfil.
    Forget {
        /// Id do perfil.
        profile: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("erro: não foi possível iniciar o runtime: {e}");
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
                println!("✔ Desconectado.");
            } else {
                println!("Nenhuma conexão ativa.");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Status => match session::status().await? {
            Some(info) => {
                println!("{}", output::connection(&info));
                Ok(ExitCode::SUCCESS)
            }
            None => {
                println!("Desconectado.");
                Ok(ExitCode::from(3))
            }
        },
        Command::Logs { follow, lines } => logs(follow, lines).await,
    }
}

fn store() -> anyhow::Result<ProfileStore> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

fn load_profile(id: &str) -> anyhow::Result<Profile> {
    match store()?.load(id) {
        Ok(p) => Ok(p),
        Err(ProfileError::NotFound(_)) => bail!(
            "perfil '{id}' não encontrado; veja `ipsec-client-cli profiles list` ou importe com \
             `ipsec-client-cli profiles import <arquivo>`"
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
        Bundle::open(dir).with_context(|| format!("pacote em {}", dir.display()))
    };
    let print_log = |log: &[String]| log.iter().for_each(|l| println!("  {l}"));
    match cmd {
        ComponentCmd::Status { bundle } => {
            let bundle = bundle.as_deref().map(open).transpose()?;
            let status = comp::status(bundle.as_ref());
            let state = match status.state {
                State::NotInstalled => "não instalado",
                State::Ready => "instalado",
                State::Outdated => "instalado, diferente do pacote (atualize)",
                State::Broken => "com problema",
            };
            println!("Componente de rede: {state}");
            if let Some(inst) = &status.installed {
                println!(
                    "  versão {} · strongSwan {} · origem {}",
                    inst.version.app.as_deref().unwrap_or("?"),
                    inst.version.strongswan.as_deref().unwrap_or("?"),
                    inst.origin.as_str()
                );
            }
            if let Some(problem) = &status.problem {
                println!("  problema: {problem}");
            }
            Ok(if status.state == State::NotInstalled {
                ExitCode::from(3)
            } else {
                ExitCode::SUCCESS
            })
        }
        ComponentCmd::Install { bundle } => {
            let bundle = open(&bundle)?;
            println!("… o sistema vai pedir a senha de administrador");
            match comp::install(&bundle, Origin::App) {
                Ok(log) => {
                    print_log(&log);
                    println!("✔ Componente de rede instalado.");
                    Ok(ExitCode::SUCCESS)
                }
                Err(comp::ComponentError::Failed { log }) => {
                    print_log(&log);
                    bail!("o instalador do componente de rede falhou")
                }
                Err(e) => Err(e.into()),
            }
        }
        ComponentCmd::Uninstall => {
            println!("… o sistema vai pedir a senha de administrador");
            match comp::uninstall() {
                Ok(log) => {
                    print_log(&log);
                    println!("✔ Componente de rede removido.");
                    Ok(ExitCode::SUCCESS)
                }
                Err(comp::ComponentError::Failed { log }) => {
                    print_log(&log);
                    bail!("a remoção do componente de rede falhou")
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
        println!("\n✘ Há problemas que impedem a conexão.");
        Ok(ExitCode::FAILURE)
    } else {
        println!("\n✔ Pronto para conectar.");
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
                    "Nenhum perfil em {}. Importe um com `ipsec-client-cli profiles import <arquivo>`.",
                    store.dir().display()
                );
            }
            for p in &list.profiles {
                println!("{:<24} {:<40} {}", p.id, p.name, p.gateway);
            }
            for (path, error) in &list.broken {
                eprintln!("⚠ ignorado {}: {error}", path.display());
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
            println!("✔ Perfil '{}' ({}) importado.", p.id, p.name);
            println!("  Próximo passo: ipsec-client-cli secrets set {}", p.id);
        }
        ProfilesCmd::Export { profile, file } => {
            store.export(&profile, &file)?;
            println!(
                "✔ Perfil exportado para {} (sem credenciais). Envie por um canal privado; \
                 a PSK do grupo vai por outro canal.",
                file.display()
            );
        }
        ProfilesCmd::Delete { profile } => {
            store.delete(&profile)?;
            println!("✔ Perfil '{profile}' removido.");
            println!(
                "  As credenciais continuam no chaveiro: ipsec-client-cli secrets forget {profile}"
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
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("não foi possível ler {}", file.display()))?;
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
            println!("Perfil gerado (sem segredos):");
            println!("  id:        {}", p.id);
            println!("  nome:      {}", p.name);
            println!("  gateway:   {}", p.gateway);
            println!(
                "  IKE:       v{}{}",
                p.ike.version,
                if p.ike.aggressive { " agressivo" } else { "" }
            );
            println!("  redes:     {}", p.child.remote_ts.join(", "));
            if p.network.dns.servers.is_empty() && p.network.dns.domains.is_empty() {
                println!("  DNS:       (nenhum; use --dns/--domain se a VPN tiver DNS interno)");
            }
            match output {
                Some(path) => {
                    write_private(&path, &ipsec_client_core::profiles::to_json(p)?)?;
                    println!(
                        "✔ Gravado em {} (0600). Importe com `profiles import`.",
                        path.display()
                    );
                }
                None => {
                    if !force && store.load(&p.id).is_ok() {
                        bail!(
                            "já existe um perfil '{}'; use --force para substituir",
                            p.id
                        );
                    }
                    store.save(p)?;
                    println!("✔ Perfil '{}' salvo.", p.id);
                    println!("  Próximo passo: ipsec-client-cli secrets set {}", p.id);
                }
            }
        }
        ProfilesCmd::Validate { file } => {
            let json = std::fs::read_to_string(&file)
                .with_context(|| format!("não foi possível ler {}", file.display()))?;
            match parse_profile(&json) {
                Ok(p) => println!("✔ Perfil '{}' válido.", p.id),
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
        .with_context(|| format!("não foi possível gravar {}", path.display()))?;
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
                    "Credenciais de '{}' (guardadas no chaveiro do sistema):",
                    profile.name
                );
                prompt::ask(&mut creds, &profile.required_credentials(), true)?;
            }
            creds.save(&store, &profile)?;
            println!("✔ Credenciais guardadas.");
        }
        SecretsCmd::Status { profile } => {
            let profile = load_profile(&profile)?;
            let store = KeyringStore::new()?;
            for kind in profile.required_credentials() {
                let value = store.get(&profile.id, kind)?;
                let shown = match (&value, kind.is_secret()) {
                    (None, _) => "ausente".to_owned(),
                    (Some(_), true) => "guardada".to_owned(),
                    (Some(v), false) => {
                        format!(
                            "guardado ({})",
                            mask(secrecy::ExposeSecret::expose_secret(v))
                        )
                    }
                };
                println!("{:<22} {shown}", describe(kind));
            }
        }
        SecretsCmd::Forget { profile } => {
            let profile = load_profile(&profile)?;
            Credentials::forget(&KeyringStore::new()?, &profile)?;
            println!("✔ Credenciais de '{}' apagadas do chaveiro.", profile.id);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_event(event: &SessionEvent, show_logs: bool) {
    match event {
        SessionEvent::State(SessionState::Preparing(step)) => println!("… {step}"),
        SessionEvent::State(SessionState::Negotiating) => println!("… negociando com o gateway"),
        SessionEvent::State(SessionState::Connected(_)) => {}
        SessionEvent::Build(line) if show_logs => println!("  [build] {line}"),
        SessionEvent::Log(line) if show_logs => println!("  {line}"),
        SessionEvent::TunnelDown => {
            println!("⚠ O túnel caiu; o charon vai tentar reconectar conforme o perfil.")
        }
        SessionEvent::TunnelUp => println!("✔ Túnel (re)estabelecido."),
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
        bail!("faltam credenciais na entrada: {}", names.join(", "));
    }
    if !missing.is_empty() {
        if !prompt::interactive() {
            bail!(
                "faltam credenciais e não há terminal para pedi-las; rode antes \
                 `ipsec-client-cli secrets set {}`",
                profile.id
            );
        }
        println!("Credenciais de '{}':", profile.name);
        prompt::ask(&mut creds, &missing, false)?;
        if remember {
            match &keyring {
                Some(store) => {
                    creds.save(store, &profile)?;
                    println!("✔ Credenciais guardadas no chaveiro.");
                }
                None => println!("⚠ Sem chaveiro: as credenciais não foram guardadas."),
            }
        }
    }

    println!("Conectando '{}' ({})…", profile.name, profile.gateway);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let connecting = session::connect(&profile, &creds, options, Some(tx));
    tokio::pin!(connecting);
    let info = loop {
        tokio::select! {
            result = &mut connecting => break result?,
            Some(event) = rx.recv() => print_event(&event, true),
            _ = tokio::signal::ctrl_c() => {
                println!("\nCancelando…");
                let _ = session::disconnect().await;
                bail!("conexão cancelada");
            }
        }
    };
    while let Ok(event) = rx.try_recv() {
        print_event(&event, verbose > 0);
    }
    println!("✔ {}", output::connection(&info));

    if detach {
        println!("\nConectado em segundo plano. Para desconectar: ipsec-client-cli disconnect");
        return Ok(ExitCode::SUCCESS);
    }
    println!("\nPressione Ctrl+C para desconectar.");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\nDesconectando…");
                session::disconnect().await?;
                println!("✔ Desconectado.");
                return Ok(ExitCode::SUCCESS);
            }
            event = rx.recv() => match event {
                Some(SessionEvent::EngineGone) | None => {
                    println!("✘ O engine parou (desconectado por outro processo ou erro).");
                    return Ok(ExitCode::FAILURE);
                }
                Some(event) => print_event(&event, verbose > 0),
            }
        }
    }
}

async fn logs(follow: bool, lines: usize) -> anyhow::Result<ExitCode> {
    let Some(profile_id) = session::active_profile().await? else {
        println!("Nenhuma conexão ativa.");
        return Ok(ExitCode::from(3));
    };
    // Mascara as identidades do perfil ativo (id do grupo e usuário XAuth).
    let mut identities: Vec<String> = Vec::new();
    if let Ok(profile) = store()?.load(&profile_id) {
        identities.extend(profile.auth.local_id.clone());
        if let Some(store) = keyring()
            && let Ok(creds) = Credentials::load(&store, &profile)
        {
            identities.extend(creds.xauth_user().map(str::to_owned));
        }
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
        for line in &all[start..] {
            println!("{}", redactor.redact(line));
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
