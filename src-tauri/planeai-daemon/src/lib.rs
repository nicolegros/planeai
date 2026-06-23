pub mod buffer;
pub mod data;
pub mod protocol;
pub mod registry;
pub mod server;
pub mod session;
pub mod transport;

use clap::Parser;
use server::DaemonServer;
use std::path::PathBuf;
use std::sync::Arc;
use transport::{default_socket_path, DaemonListener};

fn default_log_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join(".planeai").join("logs")
}

#[derive(Parser)]
#[command(
    name = "planeai-daemon",
    about = "Background daemon for planeai sessions"
)]
struct Cli {
    /// Override socket path
    #[arg(long)]
    socket_path: Option<PathBuf>,

    /// Scrollback buffer size in bytes
    #[arg(long, default_value = "1048576")]
    scrollback_bytes: usize,
}

/// Entry point for the daemon binary.
pub fn run() -> anyhow::Result<()> {
    let log_dir = default_log_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "daemon.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    let socket_path = cli.socket_path.unwrap_or_else(default_socket_path);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let listener = DaemonListener::bind(&socket_path)?;
        tracing::info!("listening on {}", socket_path.display());

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

        let socket_cleanup = socket_path.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate()).unwrap();
                let mut sigint = signal(SignalKind::interrupt()).unwrap();
                tokio::select! {
                    _ = sigterm.recv() => {}
                    _ = sigint.recv() => {}
                }
            }
            #[cfg(windows)]
            {
                tokio::signal::ctrl_c().await.unwrap();
            }
            tracing::info!("shutting down");
            let _ = shutdown_tx.send(());
            let _ = std::fs::remove_file(&socket_cleanup);
        });

        let server = Arc::new(DaemonServer::new(cli.scrollback_bytes));
        server.run(listener, shutdown_rx).await;

        let _ = std::fs::remove_file(&socket_path);
        Ok(())
    })
}
