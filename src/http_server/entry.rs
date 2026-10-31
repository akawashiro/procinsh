//! Process inspector for Linux.
//!
//! Start with the [HTTP application interface](http_server). Each subsystem's
//! **Interface** section lists its re-exported types and function signatures,
//! with links to the defining items and their source.
//!
//! # Interface
//! No items are exported. Startup delegates to [`http_server::run`].

#[path = "root.rs"]
mod http_server;
#[path = "../ipc/root.rs"]
mod ipc;
#[cfg(test)]
#[path = "../test_support.rs"]
mod test_support;

#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("procinsh supports Linux x86-64 and AArch64 only");

use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[cfg_attr(
    target_arch = "x86_64",
    command(version, about = "Read-only Linux x86-64 process inspector")
)]
#[cfg_attr(
    target_arch = "aarch64",
    command(version, about = "Read-only Linux AArch64 process inspector")
)]
struct Cli {
    /// TOML file with defaults for the options below (default: $XDG_CONFIG_HOME/procinsh/config.toml, if it exists); options given here override it
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Listen address (repeat for multiple listeners; default: 127.0.0.1:8080); non-loopback addresses require --allow-non-loopback
    #[arg(long)]
    listen: Vec<SocketAddr>,
    /// Allow non-loopback listening: exposes process memory and environment variables without authentication or TLS
    #[arg(long)]
    allow_non_loopback: bool,
    /// Accept agent links on this address and show their hosts (repeatable); non-loopback addresses also require an agent token
    #[arg(long, value_name = "ADDRESS")]
    accept_agents: Vec<SocketAddr>,
    /// Link to a hub's agent address and answer its requests for this host
    #[arg(long, value_name = "HOST:PORT")]
    connect: Option<String>,
    /// Host name shown by a hub (default: the kernel host name); letters, digits, '.', '_' and '-'
    #[arg(long, value_name = "NAME")]
    host_name: Option<String>,
    /// File holding the shared agent token
    #[arg(long, value_name = "PATH")]
    agent_token_file: Option<PathBuf>,
}

/// The config file's keys are the long option names. A relative
/// `agent-token-file` is resolved against the config file's directory.
#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct ConfigFile {
    listen: Vec<SocketAddr>,
    allow_non_loopback: bool,
    accept_agents: Vec<SocketAddr>,
    connect: Option<String>,
    host_name: Option<String>,
    agent_token_file: Option<PathBuf>,
}

fn default_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("procinsh").join("config.toml"))
}

/// Reads `--config`, or the default path when it exists. A missing explicit
/// file is an error; a missing default file means no config.
fn load_config(explicit: Option<&Path>) -> Result<ConfigFile> {
    let path = match explicit {
        Some(path) => path.to_owned(),
        None => match default_config_path() {
            Some(path) if path.exists() => path,
            _ => return Ok(ConfigFile::default()),
        },
    };
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read config file {}", path.display()))?;
    let mut config: ConfigFile =
        toml::from_str(&text).with_context(|| format!("invalid config file {}", path.display()))?;
    if let Some(token_file) = &mut config.agent_token_file
        && token_file.is_relative()
    {
        *token_file = path.parent().unwrap_or(Path::new("")).join(&*token_file);
    }
    log::info!("using config file {}", path.display());
    Ok(config)
}

/// Options given on the command line replace the config file's value; a
/// repeatable option given at least once replaces the whole list.
fn merge(cli: Cli, config: ConfigFile) -> Cli {
    let or_config = |cli: Vec<SocketAddr>, config: Vec<SocketAddr>| {
        if cli.is_empty() { config } else { cli }
    };
    let mut listen = or_config(cli.listen, config.listen);
    if listen.is_empty() {
        listen.push(SocketAddr::from(([127, 0, 0, 1], 8080)));
    }
    Cli {
        config: cli.config,
        listen,
        allow_non_loopback: cli.allow_non_loopback || config.allow_non_loopback,
        accept_agents: or_config(cli.accept_agents, config.accept_agents),
        connect: cli.connect.or(config.connect),
        host_name: cli.host_name.or(config.host_name),
        agent_token_file: cli.agent_token_file.or(config.agent_token_file),
    }
}

/// The agent token comes from a file, never from arguments or the environment,
/// because procinsh itself shows every process's command line and environment.
fn agent_token(cli: &Cli) -> Result<Option<String>> {
    let Some(path) = &cli.agent_token_file else {
        return Ok(None);
    };
    let token = std::fs::read_to_string(path)
        .with_context(|| format!("could not read agent token file {}", path.display()))?;
    let token = token.trim_end_matches(['\r', '\n']).to_owned();
    ensure!(!token.is_empty(), "the agent token is empty");
    Ok(Some(token))
}

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .format(|buffer, record| {
            writeln!(
                buffer,
                "[{} {:5} {}:{}] {}",
                buffer.timestamp_millis(),
                record.level(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(cli.config.as_deref())?;
    let cli = merge(cli, config);
    for address in &cli.listen {
        ensure!(
            address.ip().is_loopback() || cli.allow_non_loopback,
            "refusing to listen on non-loopback address {}; use --allow-non-loopback to explicitly expose process memory and environment variables without authentication or TLS",
            address
        );
    }
    let agent_token = agent_token(&cli)?;
    for address in &cli.accept_agents {
        ensure!(
            address.ip().is_loopback() || (cli.allow_non_loopback && agent_token.is_some()),
            "refusing to accept agents on non-loopback address {}; use --allow-non-loopback and an agent token (--agent-token-file)",
            address
        );
    }
    if let Some(name) = &cli.host_name {
        ensure!(
            http_server::valid_host_name(name),
            "invalid host name {name:?}: use 1-63 letters, digits, '.', '_' or '-', not \"all\""
        );
    }
    http_server::run(http_server::ServerConfig {
        listen: cli.listen,
        accept_agents: cli.accept_agents,
        connect: cli.connect,
        host_name: cli.host_name,
        agent_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("procinsh").chain(args.iter().copied()))
    }

    fn config(text: &str) -> ConfigFile {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn config_supplies_options_the_command_line_omits() {
        let merged = merge(
            parse(&[]),
            config(
                r#"
                listen = ["127.0.0.1:9090"]
                allow-non-loopback = true
                accept-agents = ["100.64.0.1:9190"]
                host-name = "hub"
                agent-token-file = "/etc/procinsh/token"
                "#,
            ),
        );
        assert_eq!(merged.listen, ["127.0.0.1:9090".parse().unwrap()]);
        assert!(merged.allow_non_loopback);
        assert_eq!(merged.accept_agents, ["100.64.0.1:9190".parse().unwrap()]);
        assert_eq!(merged.host_name.as_deref(), Some("hub"));
        assert_eq!(
            merged.agent_token_file.as_deref(),
            Some(Path::new("/etc/procinsh/token"))
        );
    }

    #[test]
    fn command_line_overrides_config() {
        let merged = merge(
            parse(&["--listen", "127.0.0.1:7070", "--host-name", "cli"]),
            config("listen = [\"127.0.0.1:9090\", \"[::1]:9090\"]\nhost-name = \"file\""),
        );
        assert_eq!(merged.listen, ["127.0.0.1:7070".parse().unwrap()]);
        assert_eq!(merged.host_name.as_deref(), Some("cli"));
    }

    #[test]
    fn listen_defaults_to_loopback_8080() {
        let merged = merge(parse(&[]), ConfigFile::default());
        assert_eq!(merged.listen, ["127.0.0.1:8080".parse().unwrap()]);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<ConfigFile>("listn = [\"127.0.0.1:9090\"]").is_err());
    }

    #[test]
    fn relative_token_file_resolves_against_the_config_directory() {
        let dir = std::env::temp_dir().join(format!("procinsh-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "agent-token-file = \"token\"\n").unwrap();
        let loaded = load_config(Some(&path));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded.unwrap().agent_token_file, Some(dir.join("token")));
    }

    #[test]
    fn missing_explicit_config_is_an_error() {
        assert!(load_config(Some(Path::new("/nonexistent/procinsh.toml"))).is_err());
    }
}
