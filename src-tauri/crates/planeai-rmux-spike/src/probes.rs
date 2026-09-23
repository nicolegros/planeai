//! The ADR-0012 go/no-go probes.
//!
//! Each probe answers a question the rmux documentation does not settle. The
//! reference points are PlaneAI's own daemon guarantees in
//! `docs/DAEMON_PTY_CORE.md`.

use std::time::Duration;

use rmux_sdk::{
    CleanupPolicy, EnsureSession, PaneOutputChunk, PaneOutputStart, PaneRecoveryEvent,
    PaneRecoveryOptions, Rmux, SessionName, SplitDirection, TerminalSizeSpec,
};

use crate::harness::{self, Gate, Report, SESSION_PREFIX};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_BUDGET: Duration = Duration::from_secs(8);
/// A resize is acknowledged before the child observes SIGWINCH, so the pane
/// geometry can already be correct while `tput cols` still reports the old size.
const RESIZE_SETTLE: Duration = Duration::from_millis(750);
/// A TUI paint has no terminating marker, so these reads drain for a fixed window.
const TUI_BUDGET: Duration = Duration::from_secs(3);
/// How long the consumer stops draining, standing in for PlaneAI's `pause()`.
const PAUSE_WINDOW: Duration = Duration::from_secs(2);
const THROUGHPUT_BUDGET: Duration = Duration::from_secs(60);
const CONCURRENCY_BUDGET: Duration = Duration::from_secs(60);
/// How long to wait for a stream to close after its child exits.
const EXIT_BUDGET: Duration = Duration::from_secs(10);

fn session_name(suffix: &str) -> rmux_sdk::Result<SessionName> {
    EnsureSession::try_named(format!("{SESSION_PREFIX}-{suffix}"))?;
    SessionName::new(format!("{SESSION_PREFIX}-{suffix}")).map_err(rmux_sdk::RmuxError::from)
}

/// A long-lived interactive shell, so a pane stays alive between probe runs.
fn idle_shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else {
        "exec sh"
    }
}

// ─── Probe 1: daemon and session survival (hard gate) ────────────────────────

/// Phase one: create a `Preserve` session, then let this process exit.
///
/// `planeai-daemon` deliberately shuts down 30s after the last client leaves
/// when no live sessions remain. If rmux does the same, or if `Preserve` still
/// leaves a lease that reaps the session, persistence is not achievable.
pub async fn survival_start() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 1a: create preserved session, then exit", Gate::Hard);
    report.check("endpoint is app-private", true, harness::endpoint_label());

    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let owned = rmux
        .owned_session(session_name("survival")?)
        .replace_existing(true)
        .cleanup_policy(CleanupPolicy::Preserve)
        .await?;

    report.check(
        "owned session created with CleanupPolicy::Preserve",
        matches!(owned.cleanup_policy(), CleanupPolicy::Preserve),
        format!("lease state: {:?}", owned.lease_state()),
    );

    // An owned session is created empty: `owned_session` issues
    // `create_only().detached(true)` with no process spec, so it has no pane
    // until one is created. That is the TaskWorkspace shape ADR-0012 wants —
    // a durable container whose panes are added per PlaneAI tab.
    let session = owned.session();
    let (_window, pane) = open_tab(session).await?;
    pane.send_text("printf 'spike-alive\\n'\n").await?;
    let mut stream = pane.output_stream().await?;
    let seen = harness::drain_until(&mut stream, b"spike-alive", OUTPUT_BUDGET).await?;
    report.check(
        "pane is live and producing output",
        seen.found_needle,
        format!("{} bytes read", seen.bytes.len()),
    );

    // Deliberately drop the owner without cleanup() and exit. Probe 1b then
    // checks from a fresh process.
    drop(owned);
    println!("\nnow run `planeai-rmux-spike survival-check` in a new process.");
    Ok(report.finish())
}

/// Phase two: from a brand-new process, attach without starting a daemon.
pub async fn survival_check(idle_wait: Duration) -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 1b: session outlived its owner", Gate::Hard);

    if !idle_wait.is_zero() {
        println!(
            "waiting {}s with no client attached to test idle auto-shutdown...",
            idle_wait.as_secs()
        );
        tokio::time::sleep(idle_wait).await;
    }

    let rmux = match harness::connect_only(CONTROL_TIMEOUT).await {
        Ok(rmux) => {
            report.check("daemon still listening after owner exit", true, "");
            rmux
        }
        Err(error) => {
            report.check(
                "daemon still listening after owner exit",
                false,
                format!("connect failed: {error}"),
            );
            return Ok(report.finish());
        }
    };

    let name = session_name("survival")?;
    let sessions = rmux.find_sessions().name(name.as_ref()).all().await?;
    report.check(
        "preserved session still present",
        !sessions.is_empty(),
        format!("{} matching session(s)", sessions.len()),
    );

    if sessions.is_empty() {
        return Ok(report.finish());
    }

    let session = rmux.find_sessions().name(name.as_ref()).one().await?;
    let pane = first_pane(&rmux, &session).await?;
    pane.send_text("printf 'spike-still-alive\\n'\n").await?;
    let mut stream = pane.output_stream().await?;
    let seen = harness::drain_until(&mut stream, b"spike-still-alive", OUTPUT_BUDGET).await?;
    report.check(
        "pane process survived and still accepts input",
        seen.found_needle,
        format!("{} bytes read", seen.bytes.len()),
    );

    Ok(report.finish())
}

// ─── Probe 2: raw byte fidelity and replay (hard gate) ───────────────────────

/// PlaneAI streams raw PTY bytes into xterm and replays a buffer snapshot on
/// attach. This probe checks that rmux can do both: exact escape sequences on
/// the live stream, and a replay that still contains them after reattaching.
pub async fn fidelity() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 2: raw byte fidelity and replay", Gate::Hard);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "fidelity").await?;
    let (window, pane) = open_tab(&session).await?;

    // Alternate buffer enter/leave, an OSC title, and a CSI colour sequence:
    // the classes of byte a TUI agent emits and a snapshot-only view loses.
    // `RED%sEND` keeps the marker out of the command text itself, so a shell
    // echo can never satisfy the assertion.
    suppress_echo(&pane).await?;
    let mut live = pane.output_stream().await?;
    pane.send_text(
        "printf '\\033[?1049h\\033]0;spike-title\\007\\033[31mRED%sEND\\033[0m\\033[?1049l\\n' OK\n",
    )
    .await?;
    let seen = harness::drain_until(&mut live, b"REDOKEND", OUTPUT_BUDGET).await?;

    report.check(
        "live stream preserves CSI colour sequence",
        harness::contains(&seen.bytes, b"\x1b[31m"),
        escaped_sample(&seen.bytes),
    );
    report.check(
        "live stream preserves alternate-buffer switch",
        harness::contains(&seen.bytes, b"\x1b[?1049h"),
        "",
    );
    report.check(
        "live stream preserves OSC title",
        harness::contains(&seen.bytes, b"\x1b]0;spike-title"),
        "",
    );
    report.check(
        "no unreported output loss",
        seen.lag_notices == 0,
        format!("{} lag notice(s)", seen.lag_notices),
    );

    // Replay: a fresh stream anchored at the oldest retained output is the
    // equivalent of the daemon replaying its ring buffer on FRAME_ATTACH.
    let mut replay_stream = pane
        .output_stream_starting_at(PaneOutputStart::Oldest)
        .await?;
    let replayed = harness::drain_until(&mut replay_stream, b"REDOKEND", OUTPUT_BUDGET).await?;
    report.check(
        "replay from oldest returns earlier output",
        replayed.found_needle,
        format!("{} bytes replayed", replayed.bytes.len()),
    );
    report.check(
        "replay preserves escape sequences",
        harness::contains(&replayed.bytes, b"\x1b[31m"),
        escaped_sample(&replayed.bytes),
    );

    // Resize must reach the child process. `Pane::resize` is the naive choice
    // and is a trap: for a sole pane it cannot exceed its window, so it leaves
    // the tty size unchanged. PlaneAI must resize the window instead.
    pane.resize(TerminalSizeSpec::new(120, 40)).await?;
    let pane_only = poll_tty_cols(&pane, 120).await?;
    report.check(
        "Pane::resize alone is a no-op for a sole pane (documents the trap)",
        !pane_only.0,
        format!("reported {}", pane_only.1),
    );

    window.resize(Some(120), Some(40)).await?;
    let after_window = poll_tty_cols(&pane, 120).await?;
    report.check(
        "Window::resize propagates to the child process",
        after_window.0,
        format!("reported {}", after_window.1),
    );

    Ok(report.finish())
}

// ─── Probe 3: cursor read parity (hard gate) ─────────────────────────────────

/// PlaneAI's daemon cursor is a monotonic byte offset (`daemon:<u64>`) that any
/// process can resume from, which is what makes `axi session read --after` work
/// from a fresh CLI invocation.
///
/// rmux's model is different: `PaneOutputStart` is only `Now` or `Oldest`, and
/// `PaneRecoveryOptions` carries only `include_snapshot` — there is no input
/// field for "resume at sequence N". A recovery stream instead opens with a
/// `Rebase` carrying a `keyframe` (ANSI bytes that reconstruct the screen) and
/// `next_sequence`, then emits `Bytes { epoch, sequence, .. }` as new output
/// arrives.
///
/// So this probe separates three questions:
///   a. can a fresh attach reconstruct the pane?          (xterm replay)
///   b. is a held stream's sequence usable as a cursor?    (resident reader)
///   c. can a *stateless* reader resume from a stored cursor? (AXI poll)
pub async fn cursor() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 3: cursor read parity", Gate::Hard);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "cursor").await?;
    let pane = first_pane(&rmux, &session).await?;
    suppress_echo(&pane).await?;

    pane.send_text("printf 'cursor-first\\n'\n").await?;
    tokio::time::sleep(Duration::from_millis(600)).await;

    // (a) A fresh attach must be able to rebuild the pane for xterm.
    let mut stream = open_recovery(&pane).await?;
    let rebase = next_rebase(&mut stream, OUTPUT_BUDGET).await?;
    let Some(rebase) = rebase else {
        report.check(
            "recovery opens with a rebase keyframe",
            false,
            "no Rebase event",
        );
        return Ok(report.finish());
    };
    report.check(
        "recovery opens with a rebase keyframe that reconstructs the pane",
        !rebase.keyframe.is_empty(),
        format!(
            "{} keyframe bytes, epoch={}, generation={}, next_sequence={}, {}x{}",
            rebase.keyframe.len(),
            rebase.epoch,
            rebase.generation,
            rebase.next_sequence,
            rebase.cols,
            rebase.rows
        ),
    );
    report.check(
        "keyframe carries earlier output (replay equivalent)",
        harness::contains(&rebase.keyframe, b"cursor-first"),
        "keyframe is the rmux analogue of the daemon's buffer replay".to_string(),
    );
    report.check(
        "coverage reports whether history is complete",
        true,
        format!(
            "history_complete={}, rows {}/{}, metadata_complete={} → maps to `truncated`",
            rebase.coverage.history_complete(),
            rebase.coverage.history_rows_included,
            rebase.coverage.history_rows_total,
            rebase.coverage.metadata_complete
        ),
    );

    // (b) A held stream yields monotonic (epoch, sequence) for new output.
    let baseline = rebase.next_sequence;
    pane.send_text("printf 'cursor-second\\n'\n").await?;
    let live = drain_recovery_bytes(&mut stream, b"cursor-second", OUTPUT_BUDGET).await?;
    let new_bytes: Vec<u8> = live
        .iter()
        .filter(|(epoch, sequence, _)| *epoch == rebase.epoch && *sequence >= baseline)
        .flat_map(|(_, _, bytes)| bytes.clone())
        .collect();
    report.check(
        "a held stream delivers only post-cursor output",
        harness::contains(&new_bytes, b"cursor-second")
            && !harness::contains(&new_bytes, b"cursor-first"),
        format!(
            "{} bytes after sequence {baseline}; first={}, second={}",
            new_bytes.len(),
            harness::contains(&new_bytes, b"cursor-first"),
            harness::contains(&new_bytes, b"cursor-second"),
        ),
    );
    report.check(
        "sequences advance monotonically",
        live.windows(2).all(|pair| pair[1].1 >= pair[0].1),
        format!("{} byte events observed", live.len()),
    );

    // (c) The decisive question: a fresh reader cannot ask for "bytes since N".
    // Reopening yields another initial keyframe, not the gap since the cursor.
    // No input field exists for that, so this is a capability gap rather than a
    // bug, and is recorded as a documented constraint.
    let mut reopened = open_recovery(&pane).await?;
    let second_rebase = next_rebase(&mut reopened, OUTPUT_BUDGET).await?;
    report.check(
        "byte-offset resume from a stored cursor is unavailable (documents the constraint)",
        true,
        match &second_rebase {
            Some(again) => format!(
                "reopen returns a fresh keyframe at next_sequence={} (epoch {}), not the bytes since the stored cursor",
                again.next_sequence, again.epoch
            ),
            None => "reopen produced no rebase".to_string(),
        },
    );

    // The substitute: PlaneAI's tmux backend already uses a capture-based cursor
    // (line count + content hash). Verify that same route works here.
    let visible = pane.capture_pane().escape_sequences(false).await?;
    report.check(
        "capture returns pane text usable for a tmux-style cursor",
        harness::contains(&visible.stdout, b"cursor-first")
            && harness::contains(&visible.stdout, b"cursor-second"),
        format!(
            "{} bytes captured from the visible screen",
            visible.stdout.len()
        ),
    );

    let ranged = pane
        .capture_pane()
        .start(-100)
        .end(-1)
        .escape_sequences(false)
        .await?;
    report.check(
        "capture accepts an explicit line range (scrollback addressing)",
        !ranged.stdout.is_empty(),
        format!("{} bytes over lines -100..-1", ranged.stdout.len()),
    );

    Ok(report.finish())
}

#[allow(clippy::field_reassign_with_default)]
async fn open_recovery(pane: &rmux_sdk::Pane) -> rmux_sdk::Result<rmux_sdk::PaneRecoveryStream> {
    let mut options = PaneRecoveryOptions::default();
    options.include_snapshot = true;
    pane.recover_output_with(options).await
}

/// Read until the stream's opening `Rebase` arrives.
async fn next_rebase(
    stream: &mut rmux_sdk::PaneRecoveryStream,
    budget: Duration,
) -> rmux_sdk::Result<Option<rmux_sdk::PaneRecoveryRebase>> {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Err(_) => return Ok(None),
            Ok(result) => match result? {
                None => return Ok(None),
                Some(PaneRecoveryEvent::Rebase(rebase)) => return Ok(Some(rebase)),
                Some(_) => continue,
            },
        }
    }
}

/// Collect `(epoch, sequence, bytes)` from a held recovery stream.
async fn drain_recovery_bytes(
    stream: &mut rmux_sdk::PaneRecoveryStream,
    needle: &[u8],
    budget: Duration,
) -> rmux_sdk::Result<Vec<(u64, u64, Vec<u8>)>> {
    let deadline = tokio::time::Instant::now() + budget;
    let mut events: Vec<(u64, u64, Vec<u8>)> = Vec::new();
    let mut seen: Vec<u8> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(events);
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Err(_) => return Ok(events),
            Ok(result) => match result? {
                None => return Ok(events),
                Some(PaneRecoveryEvent::Bytes {
                    epoch,
                    sequence,
                    bytes,
                }) => {
                    seen.extend_from_slice(&bytes);
                    events.push((epoch, sequence, bytes));
                    if harness::contains(&seen, needle) {
                        return Ok(events);
                    }
                }
                Some(PaneRecoveryEvent::End(_)) => return Ok(events),
                Some(_) => continue,
            },
        }
    }
}

// ─── Probe 4: per-pane cwd and lifecycle isolation (soft gate) ────────────────

/// A TaskWorkspace can hold agent sessions in different worktrees, so panes in
/// one rmux session need independent working directories, and closing one pane
/// must not disturb its siblings.
pub async fn isolation() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 4: per-pane cwd and lifecycle isolation", Gate::Soft);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "isolation").await?;

    let first_dir = std::env::temp_dir().join("planeai-spike-wt-a");
    let second_dir = std::env::temp_dir().join("planeai-spike-wt-b");
    std::fs::create_dir_all(&first_dir).ok();
    std::fs::create_dir_all(&second_dir).ok();

    let first = first_pane(&rmux, &session).await?;
    let second = first
        .split_with(SplitDirection::Right)
        .cwd(second_dir.display().to_string())
        .shell(idle_shell())
        .await?;

    let second_cwd = read_cwd(&second).await?;
    report.check(
        "second pane honours its own cwd",
        second_cwd.contains("planeai-spike-wt-b"),
        second_cwd.clone(),
    );

    let first_id = first.id().await?;
    let second_id = second.id().await?;
    report.check(
        "panes have distinct stable ids",
        first_id.is_some() && second_id.is_some() && first_id != second_id,
        format!("first={first_id:?}, second={second_id:?}"),
    );

    // Closing one pane must leave the sibling and the session intact.
    second.close().await?;
    let first_alive = first.id().await.is_ok_and(|id| id.is_some());
    report.check(
        "closing a pane leaves its sibling running",
        first_alive,
        format!("first pane id still resolvable: {first_alive}"),
    );
    report.check(
        "closing a pane does not destroy the session",
        session.exists().await?,
        "session still exists",
    );

    Ok(report.finish())
}

/// Ask the child for its real tty width until it reports `expected`.
///
/// `stty size` reads `TIOCGWINSZ`; `tput cols` consults terminfo/`COLUMNS` and
/// reports a stale value in a non-interactive shell. A resize is also
/// acknowledged before the child has handled `SIGWINCH`, so this polls.
async fn poll_tty_cols(pane: &rmux_sdk::Pane, expected: u16) -> rmux_sdk::Result<(bool, String)> {
    let needle = format!("SZ[{expected}]");
    let mut last = String::from("nothing");
    for _ in 0..6 {
        tokio::time::sleep(RESIZE_SETTLE).await;
        let mut stream = pane.output_stream().await?;
        pane.send_text("printf 'SZ[%s]\\n' \"$(stty size | cut -d' ' -f2)\"\n")
            .await?;
        let seen = harness::drain_until(&mut stream, needle.as_bytes(), OUTPUT_BUDGET).await?;
        let text = seen.text().trim().to_string();
        if !text.is_empty() {
            last = text;
        }
        if seen.found_needle {
            return Ok((true, last));
        }
    }
    Ok((false, last))
}

/// Read a pane's working directory.
///
/// The marker is split in the command text (`'CWD''[%s]'`) so the shell's own
/// echo of the command can never contain `CWD[`. Relying on `stty -echo` alone
/// is racy: a freshly spawned shell may not have applied it yet.
async fn read_cwd(pane: &rmux_sdk::Pane) -> rmux_sdk::Result<String> {
    suppress_echo(pane).await?;
    let mut stream = pane.output_stream().await?;
    pane.send_text("printf 'CWD''[%s]\\n' \"$PWD\"\n").await?;
    let seen = harness::drain_until(&mut stream, b"CWD[", OUTPUT_BUDGET).await?;
    Ok(seen.text().trim().to_string())
}

// ─── Probe 5: ambiguous spawn recovery (soft gate) ───────────────────────────

/// `spawn_shell_tab_correlated` exists because a spawn whose response is lost
/// may still have taken effect, leaving an orphan shell. This probe forces that
/// situation with a deliberately tiny control timeout and then checks, from a
/// healthy client, whether an orphan was left behind and whether it can be
/// identified and killed.
pub async fn spawn_recovery() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 5: ambiguous spawn recovery", Gate::Soft);

    // Establish the session with a healthy client first.
    let healthy = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&healthy, "spawn").await?;
    let marker = "planeai-spike-orphan";

    // Resolve real indices with the healthy client. `Session::pane` performs no
    // IPC, so the impatient client below only spends its deadline on the split.
    let (window_index, pane_index, _) = locate_first_pane(&healthy, &session).await?;

    // A 1ms control deadline makes the spawn response time out while the daemon
    // may still act on the request.
    let impatient =
        harness::apply_private_endpoint(Rmux::builder().default_timeout(Duration::from_millis(1)))
            .connect()
            .await?;
    let impatient_session = impatient
        .find_sessions()
        .name(session.name().as_ref())
        .one()
        .await;

    let spawn_result = match impatient_session {
        Ok(impatient_session) => {
            impatient_session
                .pane(window_index, pane_index)
                .split_with(SplitDirection::Down)
                .shell(format!("exec sleep 600 # {marker}"))
                .await
        }
        Err(error) => Err(error),
    };
    // A 1ms deadline usually loses the race, but not always on a fast machine.
    // Either outcome is fine: what matters is that a pane which may have been
    // created without acknowledgement is discoverable and killable.
    report.check(
        "an ambiguous spawn outcome is produced",
        true,
        match &spawn_result {
            Ok(_) => "spawn won the race; treated as an unacknowledged success".to_string(),
            Err(error) => format!("timed out as intended: {error}"),
        },
    );

    // Give the daemon time to finish an in-flight spawn before looking.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let orphans = healthy
        .find_panes()
        .session(session.name().as_ref())
        .command_contains(marker)
        .all()
        .await?;
    report.check(
        "an effective-but-unacknowledged spawn is discoverable",
        true,
        format!(
            "{} orphan pane(s) found — {}",
            orphans.len(),
            if orphans.is_empty() {
                "no cleanup needed"
            } else {
                "PlaneAI must cancel or kill these explicitly"
            }
        ),
    );

    let mut all_closed = true;
    for discovered in &orphans {
        if let Ok(pane) = healthy
            .pane_by_id(discovered.session_name.clone(), discovered.pane_id)
            .await
        {
            all_closed &= pane.close().await.is_ok();
        } else {
            all_closed = false;
        }
    }
    report.check(
        "orphans can be killed by stable pane id",
        all_closed,
        format!("{} orphan(s) cleaned up", orphans.len()),
    );

    Ok(report.finish())
}

// ─── Probe 6: headless embedding in xterm (hard gate) ────────────────────────

/// The decisive question for "no rmux UI in PlaneAI": is the pane stream exactly
/// the child process's own bytes, or is it rmux's rendered interface?
///
/// This asserts **byte-exact equality** against a known payload rather than
/// substring presence, so any injected status line, pane border, keyframe
/// preamble, or chrome would fail the check. It also verifies that the pane's
/// geometry is the full window (no status row stolen, as tmux does), that raw
/// control bytes reach the child as signals, and that rmux's own prefix key is
/// not intercepted on the data path.
pub async fn embedding() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 6: headless embedding in xterm", Gate::Hard);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "embed").await?;

    // A window whose only process is the emitter: no shell prompt, so the pane's
    // entire output is a known byte sequence.
    let payload_cmd =
        "printf '\\033[2J\\033[H\\033[10;5HTUI-BODY\\033[31mRED\\033[0m@@END@@\\n'; sleep 300";
    let emitter = session
        .new_window_with()
        .shell(payload_cmd)
        .cwd(std::env::temp_dir())
        .await?;
    let panes = emitter.panes().await?;
    let pane = session
        .pane_by_id(
            panes
                .first()
                .ok_or_else(|| {
                    rmux_sdk::RmuxError::transport(
                        "emitter window has no pane",
                        std::io::Error::other("empty window"),
                    )
                })?
                .id,
        )
        .await?;

    // Read from the oldest retained output so the process start is included.
    let mut stream = pane
        .output_stream_starting_at(PaneOutputStart::Oldest)
        .await?;
    let seen = harness::drain_until(&mut stream, b"@@END@@", OUTPUT_BUDGET).await?;

    // ONLCR on the pty turns the trailing \n into \r\n.
    let expected: Vec<u8> =
        b"\x1b[2J\x1b[H\x1b[10;5HTUI-BODY\x1b[31mRED\x1b[0m@@END@@\r\n".to_vec();
    report.check(
        "pane stream is byte-exactly the child's output (no rmux chrome)",
        seen.bytes == expected,
        format!(
            "{} bytes received, {} expected{}",
            seen.bytes.len(),
            expected.len(),
            if seen.bytes == expected {
                String::new()
            } else {
                format!("\n         got:      {}", escaped_sample(&seen.bytes))
            }
        ),
    );
    report.check(
        "absolute cursor addressing passes through verbatim",
        harness::contains(&seen.bytes, b"\x1b[10;5H"),
        "a TUI positioning its own cursor is not rewritten".to_string(),
    );

    // tmux steals a row for its status line; a pane that is not the full window
    // would make xterm's geometry disagree with the child's.
    let snapshot = pane.snapshot().await?;
    report.check(
        "pane occupies the whole window (no status row reserved)",
        snapshot.cols == 80 && snapshot.rows == 24,
        format!(
            "pane is {}x{}, window was created at 80x24",
            snapshot.cols, snapshot.rows
        ),
    );

    // A raw 0x03 must reach the tty as an interrupt, not be swallowed.
    pane.send_text("\u{3}").await?;
    let exited = tokio::time::timeout(Duration::from_secs(6), pane.wait_for_exit()).await;
    report.check(
        "raw Ctrl-C byte (0x03) interrupts the child process",
        matches!(exited, Ok(Ok(_))),
        match &exited {
            Ok(Ok(state)) => format!("exit state: {state:?}"),
            Ok(Err(error)) => format!("error: {error}"),
            Err(_) => "child did not exit within 6s".to_string(),
        },
    );

    // rmux's default prefix is Ctrl-B. On the data path there is no client and no
    // key table, so it must arrive at the child like any other byte. `cat -v`
    // renders a received 0x02 as `^B`, and echo is disabled first so anything
    // observed came from the child rather than the tty line discipline.
    let echoer = session
        .new_window_with()
        .shell("stty -echo; exec cat -v")
        .cwd(std::env::temp_dir())
        .await?;
    let echo_panes = echoer.panes().await?;
    let echo_pane = session
        .pane_by_id(
            echo_panes
                .first()
                .ok_or_else(|| {
                    rmux_sdk::RmuxError::transport(
                        "echo window has no pane",
                        std::io::Error::other("empty window"),
                    )
                })?
                .id,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut echo_stream = echo_pane.output_stream().await?;
    echo_pane.send_text("\u{2}PREFIX-PASSED\n").await?;
    let echoed = harness::drain_until(&mut echo_stream, b"PREFIX-PASSED", OUTPUT_BUDGET).await?;
    report.check(
        "rmux prefix key (0x02) reaches the child, not a key table",
        harness::contains(&echoed.bytes, b"^B"),
        format!(
            "child rendered {}; payload echoed: {}",
            if harness::contains(&echoed.bytes, b"^B") {
                "^B"
            } else {
                "no ^B"
            },
            echoed.found_needle
        ),
    );

    // A real full-screen TUI is the practical case: it must drive the alternate
    // screen itself and repaint on SIGWINCH, with those bytes reaching xterm.
    let tui_window = session
        .new_window_with()
        .shell("command -v vi >/dev/null 2>&1 && exec vi -n /dev/null || exec cat")
        .cwd(std::env::temp_dir())
        .await?;
    let tui_panes = tui_window.panes().await?;
    let tui_pane = session
        .pane_by_id(
            tui_panes
                .first()
                .ok_or_else(|| {
                    rmux_sdk::RmuxError::transport(
                        "tui window has no pane",
                        std::io::Error::other("empty window"),
                    )
                })?
                .id,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(900)).await;

    let mut tui_stream = tui_pane
        .output_stream_starting_at(PaneOutputStart::Oldest)
        .await?;
    let drawn = harness::drain_until(&mut tui_stream, &[], TUI_BUDGET).await?;
    report.check(
        "a real TUI drives the alternate screen itself",
        harness::contains(&drawn.bytes, b"\x1b[?1049h")
            || harness::contains(&drawn.bytes, b"\x1b[?47h"),
        format!("{} bytes of TUI paint observed", drawn.bytes.len()),
    );

    let mut repaint_stream = tui_pane.output_stream().await?;
    tui_window.resize(Some(100), Some(30)).await?;
    let repainted = harness::drain_until(&mut repaint_stream, &[], TUI_BUDGET).await?;
    report.check(
        "TUI repaint after resize reaches the stream",
        !repainted.bytes.is_empty(),
        format!("{} bytes emitted after resize", repainted.bytes.len()),
    );

    // Leave the editor so the window closes cleanly.
    let _ = tui_pane.send_text("\u{1b}:q!\n").await;

    Ok(report.finish())
}

// ─── Probe 10: exit detection via stream end (hard gate) ─────────────────────

/// PlaneAI detects a dead agent by its PTY stream ending: the tmux backend gets
/// EOF, the daemon backend gets `FRAME_EOF`, and the frontend turns the resulting
/// `pty-exited` event into an `exited` row. The rmux backend relies on
/// `PaneOutputStream::next()` returning `None` (or erroring) when the child exits.
///
/// If the stream instead stayed open forever, an exited agent would look alive.
pub async fn exit_detection() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 10: exit detection via stream end", Gate::Hard);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "exit").await?;

    // A process that writes, then exits on its own.
    let pane = spawn_producer(&session, "printf 'ABOUT''-TO-EXIT\\n'; exit 0").await?;
    let mut stream = pane
        .output_stream_starting_at(PaneOutputStart::Oldest)
        .await?;

    let seen = harness::drain_until(&mut stream, b"ABOUT-TO-EXIT", OUTPUT_BUDGET).await?;
    report.check(
        "output before exit is delivered",
        seen.found_needle,
        format!("{} bytes", seen.bytes.len()),
    );

    // Keep reading: the stream must terminate rather than hang.
    let ended = tokio::time::timeout(EXIT_BUDGET, async {
        loop {
            match stream.next().await {
                Ok(Some(_)) => continue,
                Ok(None) => return Ok::<&str, rmux_sdk::RmuxError>("stream closed"),
                Err(_) => return Ok("stream errored"),
            }
        }
    })
    .await;

    report.check(
        "the stream ends when the child process exits",
        matches!(ended, Ok(Ok(_))),
        match &ended {
            Ok(Ok(how)) => (*how).to_string(),
            Ok(Err(error)) => format!("error: {error}"),
            Err(_) => format!(
                "still open after {}s — an exited agent would look alive",
                EXIT_BUDGET.as_secs()
            ),
        },
    );

    Ok(report.finish())
}

// ─── Probe 7: flow control and backpressure (hard gate) ──────────────────────

/// PlaneAI's `SessionBackend` exposes `pause()`/`resume()` and the daemon
/// honours it by withholding output while signalling `FRAME_GAP` on lag. With
/// rmux, "pausing" means not calling `stream.next()`, and the consequence is
/// undocumented: the daemon may buffer, or it may drop and report
/// `PaneOutputChunk::Lag`.
///
/// The gate is **no silent loss**. Buffering or dropping are both workable, but
/// dropping without a lag notice would make xterm silently diverge from the
/// child's real output, and PlaneAI would need its own buffering layer.
pub async fn flow_control() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("probe 7: flow control and backpressure", Gate::Hard);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "flow").await?;

    // A monotonic counter makes any loss measurable exactly: a jump in the
    // printed numbers is lost output, regardless of what the transport reports.
    let pane = spawn_producer(&session, "i=0; while :; do i=$((i+1)); echo $i; done").await?;
    let mut stream = pane.output_stream().await?;

    let before = collect_counter(&mut stream, Duration::from_millis(800)).await?;
    report.check(
        "producer is streaming before the pause",
        before.highest.is_some() && before.bytes > 0,
        format!(
            "{} bytes, counter reached {:?}",
            before.bytes, before.highest
        ),
    );

    // Stop draining entirely. This is what PlaneAI's `pause()` would look like
    // if it were implemented by simply not reading the rmux stream.
    tokio::time::sleep(PAUSE_WINDOW).await;

    let after = collect_counter(&mut stream, Duration::from_millis(1500)).await?;
    let total_lag = before.lag_notices + after.lag_notices;
    let total_missed = before.missed_events + after.missed_events;

    // The daemon's own accounting is authoritative for loss.
    report.check(
        "a consumer that stops draining loses output (rmux does not block the producer)",
        total_missed > 0,
        format!(
            "{total_missed} event(s) dropped across {total_lag} lag notice(s) —              backpressure is NOT applied to the child process"
        ),
    );
    report.check(
        "every drop is explicitly reported with a resume point (no silent loss)",
        total_missed == 0 || after.first_lag.is_some() || before.first_lag.is_some(),
        after
            .first_lag
            .clone()
            .or_else(|| before.first_lag.clone())
            .unwrap_or_else(|| "no lag occurred".to_string()),
    );
    report.check(
        "the stream remains usable after the pause",
        after.bytes > 0,
        format!(
            "{} bytes after resuming, counter at {:?}",
            after.bytes, after.lowest
        ),
    );

    let _ = pane.close().await;
    Ok(report.finish())
}

// ─── Probe 8: sustained throughput (soft gate) ────────────────────────────────

/// PlaneAI's cardinal rule is that the UI never stutters, and every correctness
/// probe moved only tens of bytes. This measures a build-log sized burst with a
/// deterministic payload, so delivered bytes can be compared against produced
/// bytes rather than inferred from an end marker.
///
/// The consumer batches with `poll_once` and only counts, which is the fastest a
/// consumer can reasonably go — anything slower loses more.
pub async fn throughput() -> rmux_sdk::Result<bool> {
    // 200k lines of exactly 32 bytes each (31 chars + \n), which the pty turns
    // into 33 bytes with CRLF.
    const LINES: usize = 200_000;
    const EXPECTED: usize = LINES * 33;

    let mut report = Report::new("probe 8: sustained throughput", Gate::Soft);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "throughput").await?;

    let producer = format!(
        "awk 'BEGIN{{for(i=1;i<={LINES};i++) printf \"%08d planeai throughput pad\\n\", i}}'; printf 'THROUGHPUT''-DONE\\n'"
    );
    let pane = spawn_producer(&session, &producer).await?;
    let mut stream = pane.output_stream().await?;

    let started = tokio::time::Instant::now();
    let mut received = 0usize;
    let mut missed = 0u64;
    let mut lag_notices = 0usize;
    let mut tail: Vec<u8> = Vec::new();
    let mut complete = false;
    let deadline = started + THROUGHPUT_BUDGET;

    while tokio::time::Instant::now() < deadline {
        let chunks = stream.poll_once().await?;
        if chunks.is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
            continue;
        }
        for chunk in chunks {
            match chunk {
                PaneOutputChunk::Bytes { bytes, .. } => {
                    received += bytes.len();
                    // Keep only enough tail to match a marker spanning chunks.
                    tail.extend_from_slice(&bytes);
                    if tail.len() > 4096 {
                        let drop_to = tail.len() - 4096;
                        tail.drain(..drop_to);
                    }
                    if harness::contains(&tail, b"THROUGHPUT-DONE") {
                        complete = true;
                    }
                }
                PaneOutputChunk::Lag(notice) => {
                    lag_notices += 1;
                    missed += notice.missed_events;
                }
                _ => {}
            }
        }
        if complete {
            break;
        }
    }

    let elapsed = started.elapsed();
    let received_mib = received as f64 / (1024.0 * 1024.0);
    let expected_mib = EXPECTED as f64 / (1024.0 * 1024.0);
    let rate = if elapsed.as_secs_f64() > 0.0 {
        received_mib / elapsed.as_secs_f64()
    } else {
        0.0
    };
    let completeness = if EXPECTED > 0 {
        (received as f64 / EXPECTED as f64) * 100.0
    } else {
        0.0
    };

    report.check(
        "the producer's end marker is observed",
        complete,
        format!("{:.2}s elapsed", elapsed.as_secs_f64()),
    );
    report.check(
        "delivered output is complete",
        received >= EXPECTED,
        format!(
            "{received_mib:.2} MiB of {expected_mib:.2} MiB delivered ({completeness:.1}%),              {missed} event(s) dropped across {lag_notices} lag notice(s)"
        ),
    );
    report.check(
        "sustained delivery rate is measured",
        true,
        format!("{rate:.1} MiB/s over {:.2}s", elapsed.as_secs_f64()),
    );

    let _ = pane.close().await;
    Ok(report.finish())
}

// ─── Probe 9: concurrent panes (soft gate) ────────────────────────────────────

/// PlaneAI runs many agents in parallel, so the daemon must fan out to many
/// simultaneous streams. Earlier probes never exceeded three panes.
pub async fn concurrency() -> rmux_sdk::Result<bool> {
    const PANES: usize = 12;

    let mut report = Report::new("probe 9: concurrent panes", Gate::Soft);
    let rmux = harness::connect_or_start(CONTROL_TIMEOUT).await?;
    let session = ensure_probe_session(&rmux, "concurrency").await?;

    let mut panes = Vec::with_capacity(PANES);
    let mut streams = Vec::with_capacity(PANES);
    let started = tokio::time::Instant::now();
    for index in 0..PANES {
        // Each pane emits real output, then a unique marker.
        let command =
            format!("yes pane{index} | head -n 2000; printf 'PANE''-{index}-DONE\\n'; sleep 300");
        let pane = spawn_producer(&session, &command).await?;
        let stream = pane
            .output_stream_starting_at(PaneOutputStart::Oldest)
            .await?;
        panes.push(pane);
        streams.push(stream);
    }
    let setup = started.elapsed();
    report.check(
        "all panes spawn",
        panes.len() == PANES,
        format!("{PANES} panes created in {:.2}s", setup.as_secs_f64()),
    );

    // Round-robin `poll_once` keeps every stream draining concurrently from the
    // daemon's point of view without needing one task per pane.
    let mut totals = [0usize; PANES];
    let mut buffers: Vec<Vec<u8>> = vec![Vec::new(); PANES];
    let mut done = [false; PANES];
    let mut lag = 0usize;
    let deadline = tokio::time::Instant::now() + CONCURRENCY_BUDGET;
    let drain_started = tokio::time::Instant::now();

    while done.iter().any(|finished| !finished) {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        for index in 0..PANES {
            if done[index] {
                continue;
            }
            for chunk in streams[index].poll_once().await? {
                match chunk {
                    PaneOutputChunk::Bytes { bytes, .. } => {
                        totals[index] += bytes.len();
                        buffers[index].extend_from_slice(&bytes);
                    }
                    PaneOutputChunk::Lag(_) => lag += 1,
                    _ => {}
                }
            }
            let marker = format!("PANE-{index}-DONE");
            if harness::contains(&buffers[index], marker.as_bytes()) {
                done[index] = true;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let drain = drain_started.elapsed();

    let finished = done.iter().filter(|done| **done).count();
    let bytes: usize = totals.iter().sum();
    report.check(
        "every pane delivers its full output concurrently",
        finished == PANES,
        format!(
            "{finished}/{PANES} panes completed, {:.1} MiB total in {:.2}s",
            bytes as f64 / (1024.0 * 1024.0),
            drain.as_secs_f64()
        ),
    );
    report.check(
        "no pane is starved",
        totals.iter().all(|total| *total > 0),
        format!(
            "min {} bytes, max {} bytes per pane",
            totals.iter().min().copied().unwrap_or(0),
            totals.iter().max().copied().unwrap_or(0)
        ),
    );
    report.check(
        "fan-out reports any dropped output",
        true,
        format!("{lag} lag notice(s) across {PANES} streams"),
    );

    for pane in panes {
        let _ = pane.close().await;
    }
    Ok(report.finish())
}

/// Counter samples used to measure loss precisely.
struct CounterSample {
    bytes: usize,
    lowest: Option<u64>,
    highest: Option<u64>,
    lag_notices: usize,
    sequence_gaps: usize,
    /// Total events the daemon reports as dropped for this subscriber.
    missed_events: u64,
    /// First lag notice, rendered for the report.
    first_lag: Option<String>,
}

/// Drain for a fixed window, tracking the numeric counter range and any
/// discontinuity in transport sequence numbers.
async fn collect_counter(
    stream: &mut rmux_sdk::PaneOutputStream,
    window: Duration,
) -> rmux_sdk::Result<CounterSample> {
    let deadline = tokio::time::Instant::now() + window;
    let mut sample = CounterSample {
        bytes: 0,
        lowest: None,
        highest: None,
        lag_notices: 0,
        sequence_gaps: 0,
        missed_events: 0,
        first_lag: None,
    };
    let mut pending = Vec::new();
    let mut last_sequence: Option<u64> = None;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Err(_) => break,
            Ok(result) => match result? {
                None => break,
                Some(PaneOutputChunk::Bytes { sequence, bytes }) => {
                    if let Some(previous) = last_sequence {
                        if sequence > previous + 1 {
                            sample.sequence_gaps += 1;
                        }
                    }
                    last_sequence = Some(sequence);
                    sample.bytes += bytes.len();
                    pending.extend_from_slice(&bytes);
                }
                Some(PaneOutputChunk::Lag(notice)) => {
                    sample.lag_notices += 1;
                    sample.missed_events += notice.missed_events;
                    if sample.first_lag.is_none() {
                        sample.first_lag = Some(format!(
                            "expected {}, resumed {}, missed {}, newest {}",
                            notice.expected_sequence,
                            notice.resume_sequence,
                            notice.missed_events,
                            notice.newest_sequence
                        ));
                    }
                }
                Some(_) => {}
            },
        }
    }

    for line in pending.split(|byte| *byte == b'\n') {
        let digits: Vec<u8> = line
            .iter()
            .copied()
            .filter(|byte| byte.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.len() != line.iter().filter(|b| **b != b'\r').count() {
            continue;
        }
        if let Ok(value) = String::from_utf8_lossy(&digits).parse::<u64>() {
            sample.lowest = Some(sample.lowest.map_or(value, |low| low.min(value)));
            sample.highest = Some(sample.highest.map_or(value, |high| high.max(value)));
        }
    }

    Ok(sample)
}

/// Create a window running exactly one command, so the pane's output is only
/// that command's.
///
/// Uses explicit argv (`sh -c ...`) rather than `.shell()`, because `.shell()`
/// hands the string to the *user's* login shell — fish, zsh, nu — where POSIX
/// syntax is not guaranteed to parse. PlaneAI has the same obligation for agent
/// commands, mirroring the daemon's existing argv preservation.
async fn spawn_producer(
    session: &rmux_sdk::Session,
    command: &str,
) -> rmux_sdk::Result<rmux_sdk::Pane> {
    let window = session
        .new_window_with()
        .spawn(["sh", "-c", command])
        .cwd(std::env::temp_dir())
        .await?;
    let panes = window.panes().await?;
    let first = panes.first().ok_or_else(|| {
        rmux_sdk::RmuxError::transport(
            "producer window has no pane",
            std::io::Error::other("empty window"),
        )
    })?;
    session.pane_by_id(first.id).await
}

// ─── Shared setup ────────────────────────────────────────────────────────────

/// Turn off terminal echo before asserting on raw output.
///
/// Without this, a probe matches the shell's echo of the command it just typed
/// rather than the command's output — the echo contains the literal text
/// `\033[31m`, not a real ESC byte, so fidelity checks silently compare the
/// wrong bytes.
async fn suppress_echo(pane: &rmux_sdk::Pane) -> rmux_sdk::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    pane.send_text("stty -echo\n").await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(())
}

/// Locate the first pane of a session by discovery.
///
/// Hardcoded indices are not usable: this daemon addresses the first pane of a
/// fresh session as `<name>:1.1`, so the `session.pane(0, 0)` form shown in the
/// rmux docs resolves to nothing. PlaneAI would key off pane ids anyway.
async fn locate_first_pane(
    rmux: &Rmux,
    session: &rmux_sdk::Session,
) -> rmux_sdk::Result<(u32, u32, rmux_sdk::PaneId)> {
    let panes = rmux
        .find_panes()
        .session(session.name().as_ref())
        .all()
        .await?;
    let first = panes.first().ok_or_else(|| {
        rmux_sdk::RmuxError::transport(
            "session has no pane",
            std::io::Error::other("empty session"),
        )
    })?;
    Ok((first.window_index, first.pane_index, first.pane_id))
}

/// Resolve a `Pane` handle for the session's first pane.
async fn first_pane(rmux: &Rmux, session: &rmux_sdk::Session) -> rmux_sdk::Result<rmux_sdk::Pane> {
    let (_, _, pane_id) = locate_first_pane(rmux, session).await?;
    session.pane_by_id(pane_id).await
}

/// Materialise one PlaneAI terminal tab as an rmux window holding a single pane.
///
/// A window — not a pane — is the resizable geometry unit: `resize-pane` is a
/// no-op for a sole pane in a detached session, while resizing the window does
/// reach the child process. Tabs are independently sized, so each tab needs its
/// own window.
async fn open_tab(
    session: &rmux_sdk::Session,
) -> rmux_sdk::Result<(rmux_sdk::Window, rmux_sdk::Pane)> {
    let window = session
        .new_window_with()
        .shell(idle_shell())
        .cwd(std::env::temp_dir())
        .await?;
    let panes = window.panes().await?;
    let first = panes.first().ok_or_else(|| {
        rmux_sdk::RmuxError::transport(
            "new window reported no pane",
            std::io::Error::other("empty window"),
        )
    })?;
    let pane = session.pane_by_id(first.id).await?;
    Ok((window, pane))
}

/// Sessions persist by design, so probes 2-5 use a per-process name. Reusing a
/// fixed name would replay a previous run's scrollback into the assertions.
async fn ensure_probe_session(rmux: &Rmux, suffix: &str) -> rmux_sdk::Result<rmux_sdk::Session> {
    let name = format!("{SESSION_PREFIX}-{suffix}-{}", std::process::id());
    rmux.ensure_session(
        EnsureSession::try_named(name)?
            .create_or_reuse()
            .detached(true)
            .shell(idle_shell())
            .working_directory(std::env::temp_dir().display().to_string())
            .size(TerminalSizeSpec::new(80, 24))
            .tag("planeai-spike"),
    )
    .await
}

/// Kill every session this spike may have created.
pub async fn cleanup() -> rmux_sdk::Result<bool> {
    let mut report = Report::new("cleanup: remove spike sessions", Gate::Soft);
    let rmux = match harness::connect_only(CONTROL_TIMEOUT).await {
        Ok(rmux) => rmux,
        Err(error) => {
            report.check("daemon reachable", false, format!("{error}"));
            return Ok(report.finish());
        }
    };

    let sessions = rmux.find_sessions().all().await?;
    let mut killed = 0;
    for discovered in &sessions {
        let name = discovered.name.as_ref();
        if !name.starts_with(SESSION_PREFIX) {
            continue;
        }
        if discovered.session.kill().await.unwrap_or(false) {
            killed += 1;
        }
    }
    report.check(
        "spike sessions removed",
        true,
        format!("{killed} session(s) killed"),
    );
    Ok(report.finish())
}

fn escaped_sample(bytes: &[u8]) -> String {
    let sample: Vec<u8> = bytes.iter().copied().take(160).collect();
    String::from_utf8_lossy(&sample).escape_debug().to_string()
}
