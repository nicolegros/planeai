# ADR-0012: rmux as a candidate persistent session backend

## Status

Accepted — implemented as an opt-in experimental backend. The decisions below define how the rmux backend is built and what it had to prove. All ten spike probes pass against rmux 0.10.0 on macOS arm64, including the five hard gates, with the constraints recorded under "Probe results". Windows/ConPTY is out of scope and unverified. `planeai-daemon` is not retired by this ADR, and rmux does not become the default: it is selected by `session_backend: "rmux"`.

`rmux-daemon` is resolved from `PATH` rather than bundled, deliberately deferred: see "Sidecar (deferred)" below for the plan and why it is not done yet. Outstanding: there is no durable session log writer for rmux sessions, so the dogfood log viewer shows nothing for them.

## Context

PlaneAI maintains its own persistence layer, `planeai-daemon`: a bundled sidecar owning PTYs behind a JSON-line control protocol and a binary frame protocol, with a ring buffer for scrollback, explicit spawn modes, session state machine, cursor-based incremental reads, and correlated spawn cancellation. It is ours to build, debug, and support on macOS, Linux, and Windows.

rmux 0.10 is a local terminal multiplexer written in Rust that overlaps this responsibility: a daemon owning `session → window → pane` hierarchies over Unix sockets and Windows named pipes, native PTY and ConPTY, persistent detach/reattach, a typed async Rust SDK (`rmux-sdk`), explicit argv-vs-shell process launch, pane output and state streams, structured snapshots, output recovery, and tmux-compatible commands. It is dual Apache-2.0/MIT licensed, and both `rmux` and `rmux-sdk` 0.10.0 are published on crates.io.

The question is whether rmux can replace `planeai-daemon`. It cannot do so as a drop-in: PlaneAI depends on daemon-specific behaviour (raw framed PTY replay, `daemon:<offset>` cursor semantics, spawn modes keyed by PlaneAI session ID, correlated spawn cancellation, `{session_id}:{tab_index}` shell tabs). Substituting it is a backend migration, not a dependency swap.

Separately, `feat(tasks): support multiple agent sessions` introduced **TaskWorkspace** — a durable workspace per `(project_id, task_key)` owning the shared tab/split layout across every agent session, shell, editor, and diff linked to one task. This supplies the workspace boundary that an rmux session maps onto, which PlaneAI previously lacked.

## Decision

### Domain mapping

- An **rmux session** maps to a PlaneAI **TaskWorkspace**, identified by `(project_id, task_key)`.
- A PlaneAI **terminal tab/resource** maps to an rmux **window holding exactly one pane**. The pane is the process; the window is the resizable geometry unit. (Probe 2 showed `Pane::resize` is a no-op for a sole pane while `Window::resize` reaches the child, and PlaneAI tabs are independently sized full terminals rather than simultaneously visible splits.)
- Each pane is spawned with its own `cwd`. A TaskWorkspace can group agent sessions living in different worktrees, so no workspace-wide working directory is assumed.
- Lifecycle operations on one agent session (archive, delete, restart) target only that session's windows/panes. They never kill the shared rmux session or sibling panes.

### Identity and ownership

- PlaneAI SQLite remains the source of truth for task/session metadata. rmux session and pane IDs are renewable runtime handles, stored as backend state and never used to infer PlaneAI identity.
- `pty_key` (`session_id`, or `{session_id}:{tab_index}`) remains the canonical PlaneAI resource identity. It is persisted inside `task_workspaces.layout_json`, so the scheme is unchanged; rmux pane IDs are held in a backend-side map keyed by `pty_key` and never reach the frontend or persisted layout.
- A TaskWorkspace owns exactly one PlaneAI-owned rmux session, created when the workspace is first materialised and explicitly destroyed **only when its task is deleted**. Moving a task to `Done` archives its agent panes but retains the workspace and its rmux session — provided a sibling resource remains. Measured behaviour: rmux drops a session when its last window closes, so archiving the only agent on a task removes the workspace too, and the next spawn re-creates it from the same derived name. The orphan sweep relies on this, closing panes and never workspaces.
- Because tasks live in task-manager storage rather than PlaneAI's own tables, `task_workspaces` cannot foreign-key to tasks and task deletion currently emits no lifecycle event. rmux teardown is therefore an explicit, idempotent, crash-safe workflow shared by GUI, CLI, and AXI: resolve `(project_id, task_key)` → delete the rmux session → delete workspace metadata → delete the task.

### Sidecar (deferred)

**Not implemented in this slice.** rmux is resolved from `PATH`, so the backend only
works for someone who has already installed rmux — acceptable for a fourth,
opt-in, experimental backend, and revisited once it is at parity with the others.

The plan, when this is picked up: vendor the daemon **from the upstream release,
not built from source.** Upstream publishes SHA256 checksums, a Sigstore bundle,
and SLSA Build Level 2 provenance against an immutable signed release;
reproducing the build locally would trade that provenance for a longer CI matrix,
since rmux is a large Rust project and the release covers five targets.
`daemon_binary_path` in the release archive's own `share/rmux/artifact-metadata.json`
should be read rather than assumed, because the layout is not uniform: the daemon
sits at `bin/rmux-daemon` on Unix and at the archive root on Windows.

Facts already measured against the real prebuilt, worth keeping so the eventual
implementation does not re-derive them:

- The prebuilt carries three binaries — a 2.3 MB dispatcher `bin/rmux`, the 12 MB
  `bin/rmux-daemon`, and a 15 MB helper at `libexec/rmux/rmux`. Bundling the daemon
  alone is sufficient: the end-to-end suite passes with rmux absent from `PATH`
  and only an isolated `rmux-daemon` reachable via `RMUX_SDK_DAEMON_BINARY`.
- That holds only while nothing uses the SDK's `Rmux::cmd()` escape hatch. Daemon
  startup and `cmd()` read the same `RMUX_SDK_DAEMON_BINARY` override, and
  `rmux-daemon` refuses CLI invocation with `rmux-daemon is internal; launch it
through 'rmux', not directly` — reporting success while doing nothing. A caller
  that needs `cmd()` would have to bundle the 15 MB helper as `rmux`, not the
  dispatcher, which cannot find its own helper from a flat sidecar directory.
- Tauri places `externalBin` beside the executable — `Contents/MacOS/` in a macOS
  bundle, not `Contents/Resources/` — and strips the target-triple suffix, so
  `paths.rs::resolve_rmux_daemon_binary` would resolve a bundled binary through its
  exe-sibling branch, confirmed against a real built `.app`.
- The bundled daemon version and the `rmux-sdk` pin must match: rmux is not
  wire-compatible across minor versions, so a drift there is a protocol break that
  should fail the build, not degrade at runtime.

### Runtime and process model

- rmux is resolved from `PATH`, matching the existing tmux backend's requirement — the user installs rmux themselves. Bundling it as a Tauri sidecar (embedded upstream prebuilt release binaries, pinned to an exact version, SHA256-verified) is deferred until the backend is at parity with the others; see "Sidecar (deferred)" above for the plan.
- The daemon is **app-private**: its own endpoint, config, and session namespace, started via `connect_or_start`. PlaneAI never attaches to a user's own rmux server or inherits their `rmux.conf`/`tmux.conf`.
- TaskWorkspace sessions use `CleanupPolicy::Preserve` (or `detach_owned`). `KillOnOwnerExit` is reserved for genuinely ephemeral panes such as verifier commands, because a lease-based policy on workspace sessions would kill every agent when the app quits — destroying the property that motivates the backend. Probe 1 confirmed `Preserve` sessions and their panes survive the owner exiting, with no lease reaping and no idle auto-shutdown over 35s.
- Killing a session's **last** remaining session terminates the daemon and removes its socket (observed in probe work, and matching tmux and `planeai-daemon`). Every path must therefore use `connect_or_start` and treat a closed transport as "restart and retry" rather than a fatal error — deleting the final TaskWorkspace will stop the daemon.
- Integration lives in a new `planeai-rmux` library crate exposing a typed client over `rmux-sdk`. The GUI adds `PtyTarget::Rmux` implementing the existing `SessionBackend` trait, so xterm streaming, coalescing, and flow control are unchanged. `planeai-cli` depends on the same crate and connects to the app-private endpoint directly, preserving headless prompts, AXI reads, and loop orchestration while the app is closed. Concurrent writes remain serialised by the existing SQLite `prompt_locks` table.

### Data path

- xterm remains the renderer and the raw output/recovery stream remains the display transport, preserving ANSI sequences, alternate buffers, OSC titles, and interactive TUIs. Structured snapshots and surface streams are additive capabilities for AXI and automation, not the display path. Probe 6 confirms this is byte-exact: rmux acts as a process host, and none of its own interface appears in the stream.
- The AXI `session read` contract is unchanged (`text`, opaque `cursor`, `truncated`). An `rmux:<opaque>` cursor is added; unrecoverable ranges return `truncated: true`, and a pane/daemon generation change invalidates the old cursor and returns a new one rather than silently replaying or dropping output.

  Measured against rmux 0.10.0 (probe 3): rmux exposes no byte-offset resume. `PaneOutputStart` offers only `Now`/`Oldest`, and `PaneRecoveryOptions` carries only `include_snapshot`. The rmux cursor therefore follows the **tmux** strategy — `capture_pane` plus line count and content hash — rather than the daemon's `daemon:<u64>`. Truncation comes from `PaneRecoveryCoverage::history_complete()`, and `generation`/`epoch` changes signal invalidation. xterm attach uses the recovery stream's `keyframe` for replay and its `(epoch, sequence)` byte events for the live tail.

- Exit detection uses rmux pane state/exit events, mirroring `start_daemon_event_listener`. Agent state (`busy`/`stop`/`notification`) continues to come from provider hooks on the notify socket, identically to the local, tmux, and daemon backends.

### Recovery

Matching the existing daemon and tmux behaviour: unreachable rmux session or pane handles mark only the affected PlaneAI agent sessions `exited` while retaining the TaskWorkspace layout and task linkage. Selecting or restarting an exited agent recreates its pane with the recorded command, cwd, and environment, preferring the provider's `resume_command` and falling back to a fresh launch.

### Rollout

`rmux` is added as a fourth backend value alongside `local`, `tmux`, and `daemon`. The intended end state is two backends — `local` (ephemeral) and `rmux` (persistent). `daemon` is deprecated only after parity is proven; `tmux` remains while users depend on their own tmux servers. `backend` is recorded per session at creation, so existing sessions are never migrated in place: they keep running on their original backend until they exit.

### Validation before adoption

A throwaway spike (`planeai-rmux-spike`, in `src-tauri/crates/`) drives a bundled rmux through `rmux-sdk` before any production wiring, so rmux assumptions do not spread into `pty.rs`, the CLI, and AXI ahead of evidence. It is a workspace member excluded from the app's dependency graph, and it requires the `rmux` binary on PATH. It must prove:

1. **Daemon survival** — a `Preserve` session outlives the owning process; no lease reaping, no idle auto-shutdown. (`planeai-daemon` deliberately exits after 30s with no clients and no live sessions; rmux must not.)
2. **Raw byte fidelity and replay** — reattaching to a full-screen TUI agent reproduces it correctly, including alternate buffer, OSC titles, and resize; lost output is reported, never silently skipped.
3. **Cursor read parity** — incremental ANSI-stripped reads with an opaque cursor, correct `truncated` on eviction, and correct behaviour across a generation change.
4. **Per-pane cwd and lifecycle isolation** — multiple agent panes with different worktree paths in one session; closing or restarting one leaves siblings untouched.
5. **Ambiguous spawn recovery** — a timed-out pane spawn leaves no orphan process (the failure `spawn_shell_tab_correlated` exists to prevent).
6. **Headless embedding** — the pane stream is byte-exactly the child's output, with no rmux chrome, no status row stolen from the pane, raw control bytes reaching the child, and a real TUI driving its own alternate screen. PlaneAI renders in xterm and must not inherit rmux's interface.

7. **Flow control** — a consumer that stops draining must not lose output silently; whatever happens has to be observable, because PlaneAI's `SessionBackend` exposes `pause()`/`resume()`.
8. **Throughput** — a build-log sized burst must arrive complete, measured against a deterministic payload rather than inferred from an end marker.
9. **Concurrency** — many panes streaming at once, since running agents in parallel is the product.

Windows/ConPTY is deliberately out of scope for this exploration and remains unverified.

Probes 1–3, 6 and 7 are hard gates. If raw replay or cursor semantics cannot match, rmux stays an optional backend and `planeai-daemon` is not retired.

### Probe results (rmux 0.10.0 / rmux-sdk 0.10.0, macOS arm64)

All ten probes pass. The findings below are empirical and correct several assumptions made above.

| Probe                                    | Gate | Result                                                                                                                                                                |
| ---------------------------------------- | ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1 — daemon and session survival          | hard | **pass** — a `Preserve` session and its panes survive the owning process exiting; the daemon keeps listening and sessions persist through 35s with no client attached |
| 2 — raw byte fidelity and replay         | hard | **pass** — live stream carries real `ESC[31m`, `ESC[?1049h`, and OSC title bytes verbatim; replay from `Oldest` preserves them; zero lag notices                      |
| 3 — cursor read parity                   | hard | **pass with a constraint** — see below                                                                                                                                |
| 6 — headless embedding in xterm          | hard | **pass** — the pane stream is byte-exactly the child's output; see below                                                                                              |
| 4 — per-pane cwd and lifecycle isolation | soft | **pass** — panes honour independent `cwd`, have distinct stable `PaneId`s, and closing one leaves siblings and the session intact                                     |
| 7 — flow control and backpressure        | hard | **pass with a constraint** — see below                                                                                                                                |
| 5 — ambiguous spawn recovery             | soft | **pass** — an unacknowledged pane spawn is discoverable by `command_contains` and killable by stable pane id                                                          |
| 8 — sustained throughput                 | soft | **pass** — 6.29 MiB burst delivered 100% complete, zero drops, 4.1 MiB/s (release build)                                                                              |
| 9 — concurrent panes                     | soft | **pass** — 12 panes streaming simultaneously all complete, no starvation (14–16 KB each), zero lag notices                                                            |
| 10 — exit detection via stream end       | hard | **pass** — output written before exit is delivered, and the pane output stream closes when the child exits rather than hanging                                        |

### Capture shape: padded rows, bare newlines (measured)

The capture cursor was carried over from the tmux backend, which meant two tmux
behaviours were assumed rather than verified. Both were measured against rmux
0.10.0 and hold, and `capture_pads_an_unfilled_pane_and_uses_bare_newlines` in
`planeai-rmux/tests/end_to_end.rs` now pins them so a future rmux release cannot
change them silently:

- **An unfilled pane is padded to its full row count.** Trailing blank rows are
  therefore grid padding, not output, and trimming them is what keeps the line
  count tracking content. Without the trim every incremental read would look like
  a history rewrite and redeliver everything.
- **Lines are terminated with a bare `\n`.** No CR appears in a capture, so
  splitting on `lines()` and rejoining with `\n` neither loses nor invents a
  line boundary.

This is the same class of assumption as the prompt submit byte, where the tmux
`send-keys Enter` habit was mistranslated into a shell newline and every rmux
prompt silently failed to submit. Inherited semantics are checked here rather
than assumed.

Five consecutive `attended` runs pass with no flakes.

### Embedding: rmux is a process host, not a UI (probe 6)

This is the decisive check for rendering in xterm rather than adopting rmux's interface. It asserts **byte-exact equality** against a known payload rather than substring presence, so any injected status line, pane border, keyframe preamble, or rewriting would fail it.

- A pane whose only process emits a fixed 43-byte sequence produces **exactly those 43 bytes** on the output stream — no rmux chrome of any kind.
- Absolute cursor addressing (`ESC[10;5H`) passes through unmodified.
- The pane occupies the **whole window**: 80x24 requested, 80x24 delivered. Unlike tmux, no row is reserved for a status line, so xterm's geometry and the child's agree.
- Raw control bytes reach the child: `0x03` interrupts the process, and `0x02` — rmux's own default prefix — arrives at the child (rendered `^B` by `cat -v` with echo disabled) rather than being consumed by a key table. There is no client and no key bindings on the data path.
- A real `vi` session drives its own alternate screen (2013 bytes of paint) and repaints after a window resize (3041 bytes), with all of it reaching the stream.

Input goes through `Pane::send_text`, which issues `PaneInput` with `literal: true` — the `send-keys -l` equivalent. Note it takes `&str`, so the input path is UTF-8, not arbitrary bytes.

### Backpressure: rmux does not throttle the producer (probe 7)

This is the one result that dictates adapter structure. When the consumer stops draining for two seconds, the child process keeps running at full speed and the daemon **drops** output for that subscriber — 332,077 events dropped across 4,765 lag notices in the measured run. Every drop is reported as a `PaneOutputChunk::Lag` carrying `expected_sequence`, `resume_sequence`, `missed_events`, and a bounded `recent` buffer, so nothing is lost silently, and the stream stays usable afterwards.

PlaneAI's daemon behaves differently: `pause()` withholds delivery while the adapter buffers (1 MiB cap) and the ring buffer retains scrollback. So `planeai-rmux` **must not** implement `pause()` by ceasing to read the rmux stream. It has to drain continuously into its own buffer and apply pause/resume at the PlaneAI layer — structurally the same as the existing `DaemonBackend` with its flusher thread, bounded buffer, and `FlowControl`. Lag notices map onto PlaneAI's existing gap indicator.

### Consumer speed matters (probe 8)

The same 6.29 MiB burst that arrives 100% complete in a release build lost **97.6%** of its bytes in a debug build of the same consumer. rmux was not at fault either time: a slow consumer simply falls behind and gets dropped, exactly as probe 7 describes. Two consequences: the adapter's read loop is performance-critical and must not do per-chunk allocation or parsing on the hot path, and developers running dev builds should expect spurious output loss that is not a product bug.

Findings that change the design:

- **A PlaneAI tab must be an rmux window, not a sibling pane.** `Pane::resize` is a no-op for a sole pane in a detached session (tty stayed at 80 cols); `Window::resize` propagates to the child (120 cols). A window is the resizable geometry unit, and PlaneAI tabs are independently sized full terminals, so each tab needs its own window holding one pane. Sibling panes would force shared geometry.
- **The AXI cursor must be capture-based, not byte-offset based.** `PaneRecoveryOptions` carries only `include_snapshot`; there is no "resume at sequence N" input. A recovery stream opens with a `Rebase` carrying `epoch`, `generation`, `next_sequence`, and a `keyframe` of ANSI bytes that reconstructs the screen — the exact analogue of the daemon replaying its ring buffer on `FRAME_ATTACH`, and confirmed to contain earlier output. A _held_ stream then yields monotonic `(epoch, sequence)` usable as a cursor. But a _stateless_ reader (a fresh `planeai-cli axi session read --after` process) cannot ask for the bytes since a stored cursor: reopening returns another initial keyframe. The rmux cursor must therefore follow the existing **tmux** strategy (`capture_pane` plus line count and content hash), not the daemon's `daemon:<u64>` byte offset. `PaneRecoveryCoverage::history_complete()` supplies the `truncated` signal. A resident PlaneAI-side tailer writing a durable log is the only route to true byte-offset semantics, and it would reintroduce PlaneAI-side buffering.
- **Killing the last session terminates the daemon** and removes the socket, exactly as tmux does and as `planeai-daemon`'s own shutdown timer does. Deleting the final TaskWorkspace therefore stops the daemon, so every code path must use `connect_or_start` and treat a closed transport as "restart and retry", never as a fatal error. This was observed as a real `daemon closed the transport` failure mid-exchange.
- **Correlated spawn cancellation is required, not optional.** Probe 5 produced a genuine orphan process from one timed-out split, confirming the `spawn_shell_tab_correlated` problem exists on this backend too.
- **Pane indices are 1-based** (`<session>:1.1`), so the `session.pane(0, 0)` form in the rmux docs resolves to nothing. Several other documented signatures were also wrong (`SplitDirection` is `Right`/`Left`/`Up`/`Down`; `pane_by_id` takes a `SessionName`; `Pane::spawn` returns `PaneRef`). Treat the generated API reference as unreliable and read the crate source.
- **`.shell()` runs the user's login shell**, not `sh`. A POSIX command string silently failed under fish. Agent and shell commands must use explicit argv (`ProcessCommandSpec::Argv` / `spawn(["sh", "-c", ...])`), which matches the daemon's existing argv-preservation rule.
- **rmux ships two binaries.** `connect_or_start` spawns `rmux-daemon`, a separate executable from the `rmux` CLI. The SDK resolves it as `RMUX_SDK_DAEMON_BINARY` → `rmux-daemon` on PATH → a `rmux-daemon` sibling of the `rmux` binary → `rmux`. PlaneAI should ship `rmux-daemon` as a Tauri sidecar and set `RMUX_SDK_DAEMON_BINARY` to its absolute path, which avoids PATH pollution entirely. `RMUX_SDK_ENDPOINT` can carry the app-private endpoint.
- **Sessions persist across client disconnect even without `OwnedSession`**, so any test or feature that reuses a fixed session name inherits the previous run's scrollback. Probes use per-process session names for this reason.

## Consequences

- PlaneAI stops owning PTY hosting, scrollback, and multiplexing, and gains pane discovery, structured snapshots, locators, and terminal automation that the loop/AXI layers can use.
- PlaneAI takes on vendor ownership: a third-party executable inside its signed bundle, an atomic binary/SDK compatibility pair on every upgrade, and cross-platform PTY regression testing. Upstream compromise or an unavailable artifact becomes release-blocking. Package registries also lag releases (Chocolatey was still on 0.9.1 while 0.10.0 was pending), so GitHub release artifacts plus checksums are the authoritative source.
- Task deletion becomes a cross-store transaction spanning task storage, PlaneAI metadata, and a live daemon, and must be idempotent and crash-safe.
- Four concurrent backends increase surface area until `daemon` is retired. The `rmux` cursor implementation adds a third cursor format to AXI.
- The mapping is only meaningful because TaskWorkspace exists; adopting rmux before it would have required inventing an equivalent workspace entity.

## Rejected alternatives

- **Replacing `planeai-daemon` directly with rmux:** PlaneAI's frame protocol, cursor semantics, spawn modes, and shell-tab identity are daemon-specific; a direct swap is a migration disguised as a dependency change, with no parity evidence.
- **Requiring users to install rmux:** reintroduces the external-binary prerequisite that ADR-0007 removed for tmux, and is worse on Windows where there is no fallback.
- **Sharing the user's rmux daemon or session namespace:** risks name collisions, inherited user config, and cleanup of non-PlaneAI work, and makes PlaneAI's SQLite source-of-truth rule unreliable.
- **Building rmux from pinned source in release CI:** better provenance and emergency-patch latency, but rejected in favour of embedding upstream prebuilt signed artifacts to keep release engineering cost down.
- **Rendering rmux structured snapshots instead of raw bytes:** snapshots do not carry full interactive terminal semantics and would force an xterm renderer migration.
- **`CleanupPolicy::KillOnOwnerExit` for workspace sessions:** kills every agent when the app quits, contradicting the persistence requirement.
- **Mapping one rmux session per PlaneAI session, or per project:** per-session discards rmux's hierarchy and recreates the daemon glue being retired; per-project ignores the task boundary that TaskWorkspace already persists.
- **Adopting rmux quiet/foreground state for agent state:** hooks report what the agent means (finished, needs input); quiet state only reports that the terminal stopped changing. Agent-state detection stays uniform across backends.
- **Deleting the rmux session when its last pane closes, or on task `Done`:** the workspace is a durable task resource, not a cache of open panes.
