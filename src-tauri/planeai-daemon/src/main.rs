use clap::Parser;
use planeai_daemon::server::DaemonServer;
use planeai_daemon::transport::{default_socket_path, DaemonListener};
use std::path::PathBuf;
use std::sync::Arc;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let socket_path = cli.socket_path.unwrap_or_else(default_socket_path);
    let pid_path = socket_path.with_file_name("daemon.pid");

    let listener = DaemonListener::bind(&socket_path)?;
    tracing::info!("listening on {}", socket_path.display());

    // Write PID file
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    // Signal handling
    let socket_cleanup = socket_path.clone();
    let pid_cleanup = pid_path.clone();
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
        let _ = std::fs::remove_file(&pid_cleanup);
    });

    let server = Arc::new(DaemonServer::new(cli.scrollback_bytes));
    server.run(listener, shutdown_rx).await;

    // Cleanup on normal exit
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pid_path);

    Ok(())
}
