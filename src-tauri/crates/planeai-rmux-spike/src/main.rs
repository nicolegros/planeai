//! Throwaway spike for ADR-0012: can rmux replace `planeai-daemon`?
//!
//! This binary is not part of the shipped app. It drives a private rmux daemon
//! through `rmux-sdk` to answer the questions the rmux documentation leaves
//! open, before any production wiring exists.
//!
//! Requires the `rmux` binary on PATH (the SDK starts the daemon itself).
//!
//! ```text
//! planeai-rmux-spike survival-start      # then let the process exit
//! planeai-rmux-spike survival-check      # from a fresh process
//! planeai-rmux-spike fidelity            # hard gate
//! planeai-rmux-spike cursor              # hard gate
//! planeai-rmux-spike isolation
//! planeai-rmux-spike spawn-recovery
//! planeai-rmux-spike embedding           # hard gate
//! planeai-rmux-spike load                # probes 7-9 (flow control, throughput, concurrency)
//! planeai-rmux-spike attended             # every probe except survival
//! planeai-rmux-spike cleanup
//! ```
//!
//! Probes 1–3 (survival, fidelity, cursor) are hard gates: if any fails,
//! `planeai-daemon` is not retired.

mod harness;
mod probes;

use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "planeai-rmux-spike",
    about = "ADR-0012 go/no-go probes for rmux as a session backend"
)]
struct Cli {
    #[command(subcommand)]
    probe: Probe,
}

#[derive(Subcommand)]
enum Probe {
    /// Probe 1a — create a Preserve session, then exit this process.
    SurvivalStart,
    /// Probe 1b — verify the session outlived its owner. Run in a new process.
    SurvivalCheck {
        /// Seconds to idle with no client attached, to test auto-shutdown.
        #[arg(long, default_value_t = 0)]
        idle_secs: u64,
    },
    /// Probe 2 — raw byte fidelity, replay on reattach, resize propagation.
    Fidelity,
    /// Probe 3 — incremental cursor reads and truncation reporting.
    Cursor,
    /// Probe 4 — per-pane cwd and lifecycle isolation.
    Isolation,
    /// Probe 5 — no orphan process after an ambiguous spawn.
    SpawnRecovery,
    /// Probe 6 — pane stream is the child's raw bytes, not rmux's UI.
    Embedding,
    /// Probe 7 — backpressure: paused output is buffered or reported, never lost.
    FlowControl,
    /// Probe 8 — sustained throughput under a build-log sized burst.
    Throughput,
    /// Probe 9 — many concurrent panes streaming at once.
    Concurrency,
    /// Probe 10 — an exited child closes its output stream.
    ExitDetection,
    /// Every probe that does not need a process restart (2, 3, 4, 5, 6, 10).
    Attended,
    /// The load probes (7, 8, 9). Slower than `attended`.
    Load,
    /// Kill any session this spike created.
    Cleanup,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let outcome = match cli.probe {
        Probe::SurvivalStart => probes::survival_start().await,
        Probe::SurvivalCheck { idle_secs } => {
            probes::survival_check(Duration::from_secs(idle_secs)).await
        }
        Probe::Fidelity => probes::fidelity().await,
        Probe::Cursor => probes::cursor().await,
        Probe::Isolation => probes::isolation().await,
        Probe::SpawnRecovery => probes::spawn_recovery().await,
        Probe::Embedding => probes::embedding().await,
        Probe::ExitDetection => probes::exit_detection().await,
        Probe::FlowControl => probes::flow_control().await,
        Probe::Throughput => probes::throughput().await,
        Probe::Concurrency => probes::concurrency().await,
        Probe::Load => run_load().await,
        Probe::Attended => run_attended().await,
        Probe::Cleanup => probes::cleanup().await,
    };

    match outcome {
        Ok(true) => println!("\nall reported checks passed"),
        Ok(false) => {
            println!("\nsome checks failed — see the report above");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("\nprobe could not run: {error}");
            eprintln!("is the `rmux` binary on PATH?");
            std::process::exit(2);
        }
    }
}

/// Load and capacity probes, slower than the correctness suite.
async fn run_load() -> rmux_sdk::Result<bool> {
    let mut all_passed = true;
    all_passed &= probes::flow_control().await?;
    all_passed &= probes::throughput().await?;
    all_passed &= probes::concurrency().await?;
    Ok(all_passed)
}

/// Hard gates run first so a blocking failure is visible immediately.
async fn run_attended() -> rmux_sdk::Result<bool> {
    let mut all_passed = true;
    all_passed &= probes::fidelity().await?;
    all_passed &= probes::cursor().await?;
    all_passed &= probes::embedding().await?;
    all_passed &= probes::exit_detection().await?;
    all_passed &= probes::isolation().await?;
    all_passed &= probes::spawn_recovery().await?;
    Ok(all_passed)
}
