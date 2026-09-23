//! Shared harness for the ADR-0012 rmux probes: endpoint isolation, output
//! collection, and pass/fail reporting.

use std::path::PathBuf;
use std::time::Duration;

use rmux_sdk::{PaneOutputChunk, PaneOutputStream, Rmux, RmuxBuilder};

/// Session names created by the spike. Kept distinct from any real PlaneAI
/// workspace so a stray probe session is always identifiable.
pub const SESSION_PREFIX: &str = "planeai-spike";

/// The spike always talks to its own private daemon, never the developer's.
/// This mirrors the app-private endpoint rule in ADR-0012.
pub fn apply_private_endpoint(builder: RmuxBuilder) -> RmuxBuilder {
    #[cfg(windows)]
    {
        let pipe = std::env::var("PLANEAI_RMUX_SPIKE_PIPE")
            .unwrap_or_else(|_| r"\\.\pipe\planeai-rmux-spike".to_string());
        builder.windows_pipe(pipe)
    }
    #[cfg(not(windows))]
    {
        builder.unix_socket(socket_path())
    }
}

#[cfg(not(windows))]
fn socket_path() -> PathBuf {
    std::env::var("PLANEAI_RMUX_SPIKE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("planeai-rmux-spike.sock"))
}

/// Human-readable description of the endpoint, for probe output.
pub fn endpoint_label() -> String {
    #[cfg(windows)]
    {
        std::env::var("PLANEAI_RMUX_SPIKE_PIPE")
            .unwrap_or_else(|_| r"\\.\pipe\planeai-rmux-spike".to_string())
    }
    #[cfg(not(windows))]
    {
        socket_path().display().to_string()
    }
}

/// Start the private daemon if it is not already listening.
pub async fn connect_or_start(timeout: Duration) -> rmux_sdk::Result<Rmux> {
    apply_private_endpoint(Rmux::builder().default_timeout(timeout))
        .connect_or_start()
        .await
}

/// Attach without starting. Used by the survival probe: starting a daemon here
/// would mask the very failure the probe is looking for.
pub async fn connect_only(timeout: Duration) -> rmux_sdk::Result<Rmux> {
    apply_private_endpoint(Rmux::builder().default_timeout(timeout))
        .connect()
        .await
}

/// Raw bytes drained from a pane output stream.
pub struct RawOutput {
    pub bytes: Vec<u8>,
    /// Lag notices seen. A backend that silently drops output instead of
    /// reporting lag fails probe 2.
    pub lag_notices: usize,
    pub found_needle: bool,
    pub stream_ended: bool,
}

impl RawOutput {
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

/// Drain a pane output stream until `needle` appears or the budget expires.
/// Bytes are kept verbatim; the probes assert on raw sequences, not on a
/// re-rendered view.
pub async fn drain_until(
    stream: &mut PaneOutputStream,
    needle: &[u8],
    budget: Duration,
) -> rmux_sdk::Result<RawOutput> {
    let deadline = tokio::time::Instant::now() + budget;
    let mut output = RawOutput {
        bytes: Vec::new(),
        lag_notices: 0,
        found_needle: false,
        stream_ended: false,
    };

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(output);
        }
        let next = match tokio::time::timeout(remaining, stream.next()).await {
            Ok(result) => result?,
            Err(_) => return Ok(output),
        };
        match next {
            None => {
                output.stream_ended = true;
                return Ok(output);
            }
            Some(PaneOutputChunk::Bytes { bytes, .. }) => {
                output.bytes.extend_from_slice(&bytes);
                if !needle.is_empty() && contains(&output.bytes, needle) {
                    output.found_needle = true;
                    return Ok(output);
                }
            }
            Some(PaneOutputChunk::Lag(_)) => output.lag_notices += 1,
            Some(_) => {}
        }
    }
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// A probe result. Probes report every check rather than asserting, so one
/// failure still yields a full picture of what rmux does and does not provide.
pub struct Report {
    name: &'static str,
    gate: Gate,
    checks: Vec<Check>,
}

#[derive(Clone, Copy)]
pub enum Gate {
    /// Failure blocks adopting rmux as a replacement for `planeai-daemon`.
    Hard,
    /// Failure is a known cost to design around, not a blocker.
    Soft,
}

struct Check {
    label: String,
    passed: bool,
    detail: String,
}

impl Report {
    pub fn new(name: &'static str, gate: Gate) -> Self {
        Self {
            name,
            gate,
            checks: Vec::new(),
        }
    }

    pub fn check(&mut self, label: impl Into<String>, passed: bool, detail: impl Into<String>) {
        self.checks.push(Check {
            label: label.into(),
            passed,
            detail: detail.into(),
        });
    }

    /// Print the report and return true when every check passed.
    pub fn finish(self) -> bool {
        let gate = match self.gate {
            Gate::Hard => "hard gate",
            Gate::Soft => "soft gate",
        };
        println!("\n=== {} ({gate}) ===", self.name);
        let mut all_passed = true;
        for check in &self.checks {
            let mark = if check.passed { "PASS" } else { "FAIL" };
            all_passed &= check.passed;
            println!("  [{mark}] {}", check.label);
            if !check.detail.is_empty() {
                println!("         {}", check.detail);
            }
        }
        println!(
            "  → {}",
            if all_passed {
                "probe passed"
            } else {
                "probe failed"
            }
        );
        all_passed
    }
}
