use std::{error::Error, net::SocketAddr, path::PathBuf};

use agenttrace_registry::AdapterRegistry;
use agenttrace_server::{ServerConfig, serve};
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

    let store = TraceStore::open(&database_path).await?;
    store.recover_interrupted_runs().await?;
    let config = ServerConfig {
        bind: args.bind,
        allow_remote: args.allow_remote,
    };

    eprintln!("AgentTrace API: http://{}", config.bind);
    eprintln!("Database: {}", database_path.display());
    serve(config, store, AdapterRegistry::default()).await?;
    Ok(())
}
