use std::{error::Error, net::SocketAddr, path::PathBuf};

use agenttrace_redaction::Redactor;
use agenttrace_registry::AdapterRegistry;
use agenttrace_server::{ServerConfig, serve_with_redactor};
use agenttrace_storage::TraceStore;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "agenttrace-server",
    version,
    about = "Loopback-first local API for AgentTrace traces"
)]
struct Args {
    /// SQLite trace database. Defaults to .agenttrace/agenttrace.db in the current directory.
    #[arg(long)]
    db: Option<PathBuf>,

    /// Additive JSON redaction profile. Built-in safe rules always remain enabled.
    #[arg(long)]
    redaction_config: Option<PathBuf>,

    /// Socket address to bind. Non-loopback addresses require --allow-remote.
    #[arg(long, default_value = "127.0.0.1:4319")]
    bind: SocketAddr,

    /// Explicitly allow binding the API to a non-loopback interface.
    #[arg(long)]
    allow_remote: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let database_path = match args.db {
        Some(path) => path,
        None => std::env::current_dir()?
            .join(".agenttrace")
            .join("agenttrace.db"),
    };
    if let Some(parent) = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }

    let redactor = match args.redaction_config {
        Some(path) => {
            let profile = tokio::fs::read_to_string(path).await?;
            Redactor::from_profile_json(&profile)?
        }
        None => Redactor::default(),
    };
    let store = TraceStore::open(&database_path).await?;
    store.recover_interrupted_runs().await?;
    let config = ServerConfig {
        bind: args.bind,
        allow_remote: args.allow_remote,
    };

    eprintln!("AgentTrace API: http://{}", config.bind);
    eprintln!("Database: {}", database_path.display());
    serve_with_redactor(config, store, AdapterRegistry::default(), redactor).await?;
    Ok(())
}
