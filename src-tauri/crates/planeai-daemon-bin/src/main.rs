/// Thin wrapper binary — delegates to the daemon's main entrypoint.
/// This exists so `cargo build -p planeai` produces the daemon binary
/// alongside the main app (same as planeai-cli).
fn main() -> anyhow::Result<()> {
    planeai_daemon::run()
}
