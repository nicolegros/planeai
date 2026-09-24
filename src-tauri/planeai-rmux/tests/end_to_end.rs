//! End-to-end checks of the rmux backend against a real daemon.
//!
//! Exercises the same crate API the Tauri backend uses — spawn, attach, stream,
//! write, resize, capture, close — so a regression in the wiring shows up without
//! launching the desktop app. Ignored by default because it needs the `rmux`
//! binary.
//!
//! ```sh
//! cargo test -p planeai-rmux --test end_to_end -- --ignored --nocapture
//! ```

use std::time::Duration;

use planeai_rmux::{
    OutputBuffer, ResourceHandle, ResourceSpawn, RmuxClient, RmuxConfig, WorkspaceKey,
    WorkspaceName,
};

/// Each run uses its own endpoint and ids so a leftover daemon or session from a
/// previous run cannot make the result meaningless.
fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or_default()
    )
}

/// A workspace of its own per test, so one test's cleanup cannot remove another's
/// panes.
fn workspace(task: &str) -> WorkspaceName {
    WorkspaceKey::Task {
        project_id: "e2e-project".to_string(),
        task_key: task.to_string(),
    }
    .name()
}

async fn connect(runtime_dir: &std::path::Path) -> RmuxClient {
    RmuxClient::connect(RmuxConfig::app_private(runtime_dir))
        .await
        .expect("rmux daemon should start (is the `rmux` binary on PATH?)")
}

fn spawn_spec(workspace: &WorkspaceName, pty_key: &str, command: &str, cwd: &str) -> ResourceSpawn {
    ResourceSpawn {
        workspace: workspace.clone(),
        pty_key: pty_key.to_string(),
        command: command.to_string(),
        cwd: cwd.to_string(),
        env: vec!["PLANEAI_E2E=1".to_string()],
        cols: 100,
        rows: 30,
    }
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn a_resource_streams_output_and_accepts_input() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("STREAM-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            // Echo input back so a write is observable, then stay alive.
            "stty -echo; printf 'AGENT''-READY\\n'; cat -v",
            &cwd,
        ))
        .await
        .expect("spawn_resource");

    let attached = client
        .attach_resource(&workspace, handle)
        .await
        .expect("attach_resource");
    let (pane, window, mut stream) = attached.into_parts();

    // The reader drains continuously into the bounded buffer, exactly as the
    // Tauri backend does: rmux drops output for a subscriber that stops reading.
    let mut buffer = OutputBuffer::with_default_capacity();
    assert!(
        drain_until(&mut stream, &mut buffer, b"AGENT-READY").await,
        "agent startup output should arrive"
    );

    pane.send_text("HELLO-FROM-PLANEAI\n")
        .await
        .expect("send_text");
    assert!(
        drain_until(&mut stream, &mut buffer, b"HELLO-FROM-PLANEAI").await,
        "input should reach the child and be echoed back"
    );

    // Resizing addresses the window: a sole pane cannot exceed it.
    window.resize(Some(120), Some(40)).await.expect("resize");

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn agents_on_one_task_share_a_workspace_and_close_independently() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("SHARED-{}", unique_suffix());
    let workspace = workspace(&task);
    let first_dir = runtime.path().join("wt-a");
    let second_dir = runtime.path().join("wt-b");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();

    // Two agents of the same task, in different worktrees.
    let first = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent-one",
            // Report the cwd at startup: the pane then runs `cat`, which would
            // swallow any later query instead of a shell executing it.
            "stty -echo; printf 'ONE''-READY[%s]\\n' \"$PWD\"; cat",
            &first_dir.display().to_string(),
        ))
        .await
        .expect("first resource");
    let second = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent-two",
            "stty -echo; printf 'TWO''-READY[%s]\\n' \"$PWD\"; cat",
            &second_dir.display().to_string(),
        ))
        .await
        .expect("second resource");

    assert_ne!(first, second, "each resource needs its own pane");

    // Each honours its own working directory.
    let second_startup = read_startup_line(&client, &workspace, second, b"TWO-READY[").await;
    assert!(
        second_startup.contains("wt-b"),
        "second resource should run in its own worktree, got: {second_startup:?}"
    );

    // Closing one must leave the other and the workspace running.
    client
        .close_resource(&workspace, first)
        .await
        .expect("close first");

    assert!(
        client.attach_resource(&workspace, second).await.is_ok(),
        "the sibling resource must survive its neighbour closing"
    );
    assert!(
        client.workspace_exists(&workspace).await.unwrap(),
        "the workspace must outlive an individual agent"
    );
    assert!(
        client.attach_resource(&workspace, first).await.is_err(),
        "the closed resource must no longer be attachable"
    );

    // Only task deletion removes the workspace.
    client.kill_workspace(&workspace).await.expect("cleanup");
    assert!(!client.workspace_exists(&workspace).await.unwrap());
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn an_exited_resource_closes_its_stream() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("EXIT-{}", unique_suffix());
    let workspace = workspace(&task);

    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "printf 'DONE''-THEN-EXIT\\n'; sleep 3; exit 0",
            &runtime.path().display().to_string(),
        ))
        .await
        .expect("spawn_resource");

    let attached = client
        .attach_resource(&workspace, handle)
        .await
        .expect("attach_resource");
    let (_pane, _window, mut stream) = attached.into_parts();

    // Exit detection depends on the stream terminating; the frontend turns the
    // resulting `pty-exited` event into an `exited` row.
    let closed = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match stream.next().await {
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => return,
            }
        }
    })
    .await;

    assert!(
        closed.is_ok(),
        "stream must end when the agent exits, or an exited agent looks alive"
    );

    let _ = client.kill_workspace(&workspace).await;
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn capture_backs_incremental_cursor_reads() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("CURSOR-{}", unique_suffix());
    let workspace = workspace(&task);

    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "stty -echo; cat",
            &runtime.path().display().to_string(),
        ))
        .await
        .expect("spawn_resource");

    let attached = client
        .attach_resource(&workspace, handle)
        .await
        .expect("attach_resource");
    let (pane, _window, _stream) = attached.into_parts();
    tokio::time::sleep(Duration::from_millis(600)).await;

    pane.send_text("FIRST-LINE\n").await.expect("send first");
    tokio::time::sleep(Duration::from_millis(600)).await;

    let captured = client
        .capture_resource_text(&workspace, handle)
        .await
        .expect("capture");
    let first = planeai_core::capture_cursor::read_after("rmux", &captured, "rmux:0:0", 0)
        .expect("first read");
    assert!(
        first.text.contains("FIRST-LINE"),
        "first read should include earlier output, got: {:?}",
        first.text
    );
    assert!(!first.truncated);

    pane.send_text("SECOND-LINE\n").await.expect("send second");
    tokio::time::sleep(Duration::from_millis(600)).await;

    // The returned cursor must yield only what arrived since.
    let captured = client
        .capture_resource_text(&workspace, handle)
        .await
        .expect("capture again");
    let second = planeai_core::capture_cursor::read_after("rmux", &captured, &first.cursor, 0)
        .expect("second read");
    assert!(
        second.text.contains("SECOND-LINE"),
        "second read should include new output, got: {:?}",
        second.text
    );
    assert!(
        !second.text.contains("FIRST-LINE"),
        "second read must not repeat delivered output, got: {:?}",
        second.text
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn removing_absent_things_is_not_an_error() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let workspace = workspace(&format!("MISSING-{}", unique_suffix()));

    // Cleanup runs for resources whose host may already be gone; that is success,
    // not failure, because the daemon exits with its last session.
    client
        .kill_workspace(&workspace)
        .await
        .expect("killing an absent workspace should succeed");
    client
        .close_resource(&workspace, ResourceHandle::from_u32(4_242))
        .await
        .expect("closing an absent resource should succeed");
    assert!(!client.workspace_exists(&workspace).await.unwrap());
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn live_panes_report_the_workspace_holding_them() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("LIVE-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    let agent = client
        .spawn_resource(&spawn_spec(&workspace, "agent", "sleep 600", &cwd))
        .await
        .expect("spawn agent");
    let shell = client
        .spawn_resource(&spawn_spec(&workspace, "agent:1", "sleep 600", &cwd))
        .await
        .expect("spawn shell tab");

    let live = client.live_panes().await.expect("live_panes");

    // The orphan sweep addresses a close by workspace, so a pane that cannot name
    // its workspace cannot be collected.
    let mine: Vec<&planeai_rmux::LivePane> = live
        .iter()
        .filter(|pane| pane.workspace_key == workspace.as_str())
        .collect();
    assert_eq!(mine.len(), 2, "both panes should be reported: {live:?}");
    for pane in &mine {
        assert_eq!(
            planeai_rmux::WorkspaceName::from_stored(&pane.workspace_key).as_ref(),
            Some(&workspace),
            "the reported name must parse back to the workspace that was spawned"
        );
    }
    let mut reported: Vec<u32> = mine.iter().map(|pane| pane.pane_id).collect();
    reported.sort_unstable();
    let mut expected = vec![agent.as_u32(), shell.as_u32()];
    expected.sort_unstable();
    assert_eq!(reported, expected);

    // Closing one pane removes exactly that pane from the live set.
    client
        .close_resource(&workspace, shell)
        .await
        .expect("close shell tab");
    let live = client.live_panes().await.expect("live_panes after close");
    let mine: Vec<u32> = live
        .iter()
        .filter(|pane| pane.workspace_key == workspace.as_str())
        .map(|pane| pane.pane_id)
        .collect();
    assert_eq!(mine, vec![agent.as_u32()]);

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn a_client_whose_daemon_exited_reports_it_as_recoverable() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("GONE-{}", unique_suffix());
    let workspace = workspace(&task);

    client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "sleep 600",
            &runtime.path().display().to_string(),
        ))
        .await
        .expect("spawn_resource");

    // Removing the only workspace stops the daemon, which leaves this client
    // holding a dead transport — the state a cached client reaches in the app.
    client.kill_workspace(&workspace).await.expect("kill");

    let error = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "sleep 600",
            &runtime.path().display().to_string(),
        ))
        .await
        .expect_err("a dead transport must surface as an error");

    // Classified as recoverable so callers reconnect and retry rather than
    // reporting a fault to the user.
    assert!(
        error.is_daemon_gone(),
        "a closed transport must be recoverable, got: {error}"
    );

    // A fresh client restarts the daemon and works, which is what the app's
    // retry does after invalidating its cached connection.
    let reconnected = connect(runtime.path()).await;
    let handle = reconnected
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "stty -echo; printf 'BACK''-UP\\n'; cat",
            &runtime.path().display().to_string(),
        ))
        .await
        .expect("reconnected client should spawn");

    let attached = reconnected
        .attach_resource(&workspace, handle)
        .await
        .expect("attach after reconnect");
    let (_pane, _window, mut stream) = attached.into_parts();
    let mut buffer = OutputBuffer::with_default_capacity();
    assert!(
        drain_until(&mut stream, &mut buffer, b"BACK-UP").await,
        "the reconnected resource should stream output"
    );

    reconnected
        .kill_workspace(&workspace)
        .await
        .expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn every_resource_receives_its_environment_not_just_the_first() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("ENV-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    // The first resource creates the session and gets its environment through
    // `ensure_session`.
    let first = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "stty -echo; printf 'ONE''[%s]\\n' \"$PLANEAI_E2E\"; cat",
            &cwd,
        ))
        .await
        .expect("first resource");

    // A later resource is a new window. It must be given the environment
    // explicitly — otherwise a shell tab runs without PlaneAI's augmented PATH
    // and a configured editor is not found.
    let second = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent:1",
            "stty -echo; printf 'TWO''[%s]\\n' \"$PLANEAI_E2E\"; cat",
            &cwd,
        ))
        .await
        .expect("second resource");

    let first_line = read_startup_line(&client, &workspace, first, b"ONE[").await;
    assert!(
        first_line.contains("ONE[1]"),
        "first resource should see PLANEAI_E2E=1, got: {first_line:?}"
    );

    let second_line = read_startup_line(&client, &workspace, second, b"TWO[").await;
    assert!(
        second_line.contains("TWO[1]"),
        "a later resource must receive the same environment, got: {second_line:?}"
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn a_submitted_prompt_delivers_carriage_return_to_a_raw_mode_pane() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("SUBMIT-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    // A raw-mode reader is the whole point: in canonical mode the tty driver maps
    // CR to LF on input, so both bytes would appear to work and the test would
    // prove nothing. `stty raw` is what an agent TUI does, and there Enter is
    // literally 0x0D. Dump the bytes as they arrive rather than interpreting them.
    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "stty raw -echo; printf 'READY\\n'; head -c 3 | od -An -tx1; stty sane; printf '\\nEND\\n'",
            &cwd,
        ))
        .await
        .expect("spawn raw-mode reader");

    let attached = client
        .attach_resource(&workspace, handle)
        .await
        .expect("attach");
    let (_pane, _window, mut stream) = attached.into_parts();
    let mut buffer = OutputBuffer::with_default_capacity();
    assert!(
        drain_until(&mut stream, &mut buffer, b"READY").await,
        "the pane never reached raw mode"
    );

    client
        .submit_text(&workspace, handle, "ab")
        .await
        .expect("submit_text");

    assert!(
        drain_until(&mut stream, &mut buffer, b"END").await,
        "the reader never finished; output so far: {}",
        String::from_utf8_lossy(&buffer.take())
    );
    let output = String::from_utf8_lossy(&buffer.take()).to_string();
    let hex: String = output.split_whitespace().collect::<Vec<_>>().join(" ");

    // "ab" then the submit byte: 0x61 0x62 0x0d. A regression to `\n` shows as 0a.
    assert!(
        hex.contains("61 62 0d"),
        "expected the prompt to end in CR (0d), got: {output}"
    );
    assert!(
        !hex.contains("61 62 0a"),
        "prompt was submitted with LF, which a raw-mode agent does not read as Enter: {output}"
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn respawning_a_resource_adopts_its_pane_instead_of_creating_a_second() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("IDEMPOTENT-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    // `with_retry` re-runs an operation when the transport dies, and a transport
    // failure cannot tell "the daemon never saw it" from "the daemon spawned it and
    // the reply was lost". Repeating the spawn stands in for that retry: it must
    // adopt the existing pane, or the first one becomes an orphan (ADR-0012 probe 5).
    let first = client
        .spawn_resource(&spawn_spec(&workspace, "agent", "sleep 600", &cwd))
        .await
        .expect("first spawn");
    let again = client
        .spawn_resource(&spawn_spec(&workspace, "agent", "sleep 600", &cwd))
        .await
        .expect("repeat spawn");

    assert_eq!(first, again, "a repeated spawn must adopt the same pane");
    let live: Vec<u32> = client
        .live_panes()
        .await
        .expect("live_panes")
        .into_iter()
        .filter(|pane| pane.workspace_key == workspace.as_str())
        .map(|pane| pane.pane_id)
        .collect();
    assert_eq!(
        live,
        vec![first.as_u32()],
        "a second pane was created: {live:?}"
    );

    // The same must hold for a shell tab joining an existing workspace, which
    // takes the other branch of spawn_resource.
    let tab = client
        .spawn_resource(&spawn_spec(&workspace, "agent:1", "sleep 600", &cwd))
        .await
        .expect("spawn shell tab");
    let tab_again = client
        .spawn_resource(&spawn_spec(&workspace, "agent:1", "sleep 600", &cwd))
        .await
        .expect("repeat shell tab spawn");

    assert_eq!(tab, tab_again);
    assert_ne!(tab, first, "the shell tab must be its own pane");
    let mut live: Vec<u32> = client
        .live_panes()
        .await
        .expect("live_panes")
        .into_iter()
        .filter(|pane| pane.workspace_key == workspace.as_str())
        .map(|pane| pane.pane_id)
        .collect();
    live.sort_unstable();
    let mut expected = vec![first.as_u32(), tab.as_u32()];
    expected.sort_unstable();
    assert_eq!(live, expected, "workspace should hold exactly two panes");

    // find_resource is the correlator the retry path depends on.
    assert_eq!(
        client.find_resource(&workspace, "agent").await.unwrap(),
        Some(first)
    );
    assert_eq!(
        client.find_resource(&workspace, "agent:1").await.unwrap(),
        Some(tab)
    );
    assert_eq!(
        client
            .find_resource(&workspace, "never-spawned")
            .await
            .unwrap(),
        None
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn find_resource_reports_nothing_for_a_workspace_that_does_not_exist() {
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let workspace = workspace(&format!("ABSENT-{}", unique_suffix()));

    // An absent workspace is an answer, not a failure: the caller is about to
    // create it, and a spawn must not be blocked by the correlation query.
    assert_eq!(
        client.find_resource(&workspace, "agent").await.unwrap(),
        None
    );
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn capture_pads_an_unfilled_pane_and_uses_bare_newlines() {
    // Measures the two capture assumptions the cursor strategy inherited from
    // tmux, rather than trusting that rmux shares them. If either is wrong the
    // line-count-and-hash cursor silently misbehaves: padding that is kept holds
    // the line count constant while content changes, and a CR or CRLF terminator
    // would change how many lines the cursor thinks it delivered.
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("CAPFORM-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "printf 'ALPHA\\nBETA\\n'; sleep 600",
            &cwd,
        ))
        .await
        .expect("spawn");

    // Wait for both lines to land before capturing.
    let printed = read_startup_line(&client, &workspace, handle, b"BETA").await;
    assert!(printed.contains("BETA"), "pane never printed: {printed:?}");

    let raw = client
        .capture_raw_for_test(&workspace, handle)
        .await
        .expect("raw capture");

    // 1. Line endings: the cursor splits on `lines()`, which treats a lone CR as
    //    part of the line. A CR-terminated capture would break line counting.
    assert!(
        !raw.contains('\r'),
        "capture contains CR, so the cursor's line splitting is unsafe: {:?}",
        raw.chars().take(200).collect::<String>()
    );

    // 2. Padding: the pane is 30 rows (spawn_spec) holding two lines of output, so
    //    a fixed-height grid capture must come back with trailing blank rows. If
    //    rmux did NOT pad, trimming them would be discarding real content.
    let trailing_blanks = raw
        .lines()
        .rev()
        .take_while(|line| line.trim().is_empty())
        .count();
    assert!(
        trailing_blanks > 0,
        "capture was not padded, so trailing-blank trimming is unjustified: {raw:?}"
    );

    // And the trimmed form the cursor actually consumes keeps the content.
    let trimmed = client
        .capture_resource_text(&workspace, handle)
        .await
        .expect("capture");
    assert!(
        trimmed.contains("ALPHA") && trimmed.contains("BETA"),
        "{trimmed:?}"
    );
    assert!(
        !trimmed.ends_with('\n') && !trimmed.ends_with(' '),
        "trimmed capture still ends in blank padding: {trimmed:?}"
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires the rmux binary"]
async fn attaching_replays_earlier_output_through_a_recovery_keyframe() {
    // ADR-0012 requires attach to replay through the recovery stream's keyframe
    // rather than raw retained bytes. The distinguishing property: the stream opens
    // with an Initial rebase whose keyframe already reconstructs the screen, so
    // output produced before the terminal ever attached is present.
    let runtime = tempfile::tempdir().unwrap();
    let client = connect(runtime.path()).await;
    let task = format!("REPLAY-{}", unique_suffix());
    let workspace = workspace(&task);
    let cwd = runtime.path().display().to_string();

    let handle = client
        .spawn_resource(&spawn_spec(
            &workspace,
            "agent",
            "printf 'BEFORE-ATTACH\\n'; sleep 600",
            &cwd,
        ))
        .await
        .expect("spawn");

    // Let the output land before attaching at all.
    tokio::time::sleep(Duration::from_millis(600)).await;

    let attached = client
        .attach_resource(&workspace, handle)
        .await
        .expect("attach");
    let (_pane, _window, mut stream) = attached.into_parts();

    let first = stream
        .next()
        .await
        .expect("stream open")
        .expect("an event is delivered");
    let rmux_sdk::PaneRecoveryEvent::Rebase(rebase) = first else {
        panic!("a recovery stream must open with a rebase, got {first:?}");
    };
    assert_eq!(
        rebase.reason,
        rmux_sdk::PaneRecoveryRebaseReason::Initial,
        "the opening rebase should be the initial one"
    );
    assert_eq!(rebase.cols, 100, "keyframe should report the spawned width");
    assert_eq!(rebase.rows, 30, "keyframe should report the spawned height");

    // The keyframe alone must carry output produced before this attach existed.
    let keyframe = String::from_utf8_lossy(&rebase.keyframe);
    assert!(
        keyframe.contains("BEFORE-ATTACH"),
        "keyframe did not reconstruct earlier output: {keyframe:?}"
    );
    // Coverage is what the cursor's truncation signal is built on.
    assert!(
        rebase.coverage.history_complete(),
        "a short-lived pane's history should be complete: {:?}",
        rebase.coverage
    );

    client.kill_workspace(&workspace).await.expect("cleanup");
}

/// Replay a resource's output from the start and return what it printed.
async fn read_startup_line(
    client: &RmuxClient,
    workspace: &WorkspaceName,
    handle: ResourceHandle,
    needle: &[u8],
) -> String {
    let attached = client
        .attach_resource(workspace, handle)
        .await
        .expect("attach for startup line");
    let (_pane, _window, mut stream) = attached.into_parts();
    let mut buffer = OutputBuffer::with_default_capacity();
    drain_until(&mut stream, &mut buffer, needle).await;
    String::from_utf8_lossy(&buffer.take()).to_string()
}

/// Drain into the buffer until `needle` appears in the accumulated output.
///
/// Mirrors the production reader in `pty.rs`: a rebase replaces what has been
/// seen rather than appending to it, because epochs must not be stitched together.
async fn drain_until(
    stream: &mut rmux_sdk::PaneRecoveryStream,
    buffer: &mut OutputBuffer,
    needle: &[u8],
) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut seen: Vec<u8> = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let next = match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Ok(next)) => next,
            Ok(Err(_)) | Err(_) => return false,
        };
        let fed: Vec<u8> = match next {
            None => return false,
            Some(rmux_sdk::PaneRecoveryEvent::Rebase(rebase)) => {
                if rebase.reason == rmux_sdk::PaneRecoveryRebaseReason::Initial {
                    buffer.push(&rebase.keyframe);
                } else {
                    seen.clear();
                    buffer.replace_with_keyframe(&rebase.keyframe);
                }
                rebase.keyframe
            }
            Some(rmux_sdk::PaneRecoveryEvent::Bytes { bytes, .. }) => {
                buffer.push(&bytes);
                bytes
            }
            Some(rmux_sdk::PaneRecoveryEvent::End(_)) => return false,
            Some(_) => continue,
        };
        seen.extend_from_slice(&fed);
        if seen.windows(needle.len()).any(|window| window == needle) {
            return true;
        }
    }
}
