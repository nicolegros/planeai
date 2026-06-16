use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod protocol;
mod ring_buffer;
mod server;
mod session;

fn socket_path() -> PathBuf {
    let dir = daemon_dir();
    std::fs::create_dir_all(&dir).ok();
    dir.join("daemon.sock")
}

fn daemon_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join("Library/Application Support/ca.nicolegros.planeai")
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{home}\\AppData\\Roaming")
        });
        PathBuf::from(base).join("ca.nicolegros.planeai")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base =
            std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
        PathBuf::from(base).join("ca.nicolegros.planeai")
    }
}

fn pid_path() -> PathBuf {
    daemon_dir().join("daemon.pid")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    // Write PID file
    let pid = std::process::id();
    std::fs::write(pid_path(), pid.to_string()).ok();

    let sock = socket_path();
    // Clean stale socket
    let _ = std::fs::remove_file(&sock);

    tracing::info!(?sock, pid, "planeai-daemon starting");

    server::run(sock).await;

    // Cleanup
    let _ = std::fs::remove_file(socket_path());
    let _ = std::fs::remove_file(pid_path());
    tracing::info!("planeai-daemon exiting");
}
