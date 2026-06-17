# PlaneAI Iced Terminal Spike

Native Rust terminal renderer using **Iced 0.14** + **alacritty_terminal 0.26**.
Supports replay benchmarking, live interactive shell, command mode, and automated input-latency benchmarking.

## Build

```bash
cd src-tauri
cargo build --release -p planeai-iced-spike
```

Binary: `src-tauri/target/release/planeai-iced-spike`

## Modes

### Replay Mode

Feeds a raw `.ansi` fixture file into alacritty_terminal at configurable chunk size/interval.
Emits JSONL metrics and exits.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --replay bench/fixtures/mixed-agent-like.ansi \
  --cols 120 --rows 40 \
  --chunk-size 16384 --chunk-interval-ms 4 \
  --metrics bench/results/iced-alacritty_replay.jsonl \
  --backend iced-alacritty --exit-when-done
```

### Live Shell Mode

Opens a real PTY with your `$SHELL`. Interactive terminal with keyboard input, paste, and resize.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell --cols 120 --rows 40 \
  --metrics bench/results/iced-alacritty_shell.jsonl \
  --backend iced-alacritty
```

### Command Mode

Runs a specific command inside the PTY. Exits when the command completes.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --command "bash -lc 'for i in {1..1000}; do echo line-\$i; done'" \
  --cols 120 --rows 40 \
  --metrics bench/results/smoke-command.jsonl \
  --backend iced-alacritty --exit-when-done
```

### Input Benchmark Mode

Injects synthetic keystrokes at intervals while shell output is streaming.
Measures input write latency under load.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell --input-benchmark \
  --input-interval-ms 50 --input-events 100 \
  --cols 120 --rows 40 \
  --metrics bench/results/iced-alacritty_input-latency.jsonl \
  --backend iced-alacritty --exit-when-done
```

With flood output for stress testing:

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell --input-benchmark \
  --flood-command "python3 bench/flood-output.py" \
  --input-interval-ms 50 --input-events 100 \
  --cols 120 --rows 40 \
  --metrics bench/results/iced-alacritty_input-flood.jsonl \
  --backend iced-alacritty --exit-when-done
```

Or using `--command` directly:

```bash
cargo run --release --bin planeai-iced-spike -- \
  --command "python3 bench/flood-output.py" \
  --input-benchmark \
  --input-interval-ms 50 --input-events 100 \
  --cols 120 --rows 40 \
  --metrics bench/results/iced-alacritty_input-flood.jsonl \
  --backend iced-alacritty --exit-when-done
```

## Flood Output Script

`bench/flood-output.py` emits continuous colored terminal output until Ctrl-C:

```bash
python3 bench/flood-output.py
```

Features: deterministic colored lines, carriage-return progress updates every 50 lines, clean KeyboardInterrupt handling.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--replay <path>` | Raw ANSI fixture file for replay mode |
| `--shell` | Launch interactive shell |
| `--command <cmd>` | Run specific command in PTY |
| `--flood-command <cmd>` | Command to run in PTY for input benchmark (with --shell) |
| `--cols <n>` | Terminal columns (default: 120) |
| `--rows <n>` | Terminal rows (default: 40) |
| `--chunk-size <bytes>` | Bytes per replay tick (default: 16384) |
| `--chunk-interval-ms <ms>` | Delay between chunks, 0=maxspeed (default: 4) |
| `--metrics <path>` | JSONL output path |
| `--backend <name>` | Backend identifier (default: iced-alacritty) |
| `--exit-when-done` | Exit after replay/benchmark completes |
| `--snapshot <path>` | Write visible text after replay |
| `--font-size <n>` | Font size |
| `--scrollback-lines <n>` | Scrollback buffer size |
| `--max-runtime-ms <ms>` | Safety timeout |
| `--warmup-ms <ms>` | Frames before this threshold are excluded from p95/p99 (default: 500) |
| `--input-benchmark` | Enable synthetic input injection |
| `--input-interval-ms <ms>` | Interval between synthetic inputs (default: 50) |
| `--input-events <n>` | Number of synthetic inputs (default: 100) |
| `--output-queue-policy <p>` | Queue policy: `block` (default, lossless) or `drop_oldest` (lossy, stress testing only) |

## Input Support

### Keyboard

- Printable characters (typing)
- Enter, Backspace, Tab, Escape
- Ctrl-C, Ctrl-D, Ctrl-L, Ctrl+any letter
- Arrow keys (Up/Down/Left/Right)
- Home / End / PageUp / PageDown
- Delete / Insert

### Paste

- **Cmd+V** (macOS) / **Ctrl+V** (Linux/Windows)
- Plain text pasted directly to PTY
- Multi-line paste works (newlines preserved)
- Large paste does not freeze UI
- Emits `input_event_received` and `input_write_done` with `input_kind: "paste"`

### Window Resize

- Window resize events are captured from Iced
- Terminal cols/rows recomputed from window dimensions
- alacritty_terminal state resized
- PTY resized via `Shell::resize()`
- Emits `pty_resize` metric event

## Warmup Behavior

Frame delta metrics (p95, p99) are computed only from samples collected **after** the warmup period (default 500ms from boot). This prevents the first-frame startup spike from polluting percentile metrics.

Summary fields:
- `warmup_ms` — threshold used
- `frame_samples_total` — all frame deltas recorded
- `frame_samples_after_warmup` — samples used for p95/p99 computation

## Queue / Backpressure Policy

PTY output is read on a background thread into a **bounded 512KB buffer**.

**Default policy: `block` (lossless)**

The PTY reader thread blocks (via Condvar) when the buffer is full, waiting for the UI poll to drain it. This guarantees zero bytes are ever dropped, preserving terminal ANSI state integrity.

| Property | Value |
|----------|-------|
| `output_queue_capacity_bytes` | 524288 (512KB) |
| `output_queue_policy` (default) | `block` |
| Behavior when full | PTY reader blocks until UI drains |
| `output_bytes_dropped` | Always 0 in block mode |
| `producer_block_count` | Number of times the reader thread blocked |
| `producer_block_duration_ms` | Total time spent blocked |

**Alternative policy: `drop_oldest` (stress testing only)**

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell --output-queue-policy drop_oldest ...
```

⚠️ **Warning:** `drop_oldest` silently discards terminal bytes when the buffer is full. This can corrupt ANSI state, lose command output, and produce garbled display. Use only for stress testing and flood throughput measurement — never for production terminal sessions.

The Iced UI polls every 16ms, drains the entire buffer, parses through alacritty_terminal, and re-renders. Under normal workloads the buffer never fills and the reader never blocks.

## Metrics Emitted

### Replay Mode Events

`replay_start`, `chunk_sent`, `parse_batch`, `render_frame`, `frame_sample`, `backlog_sample`, `replay_done`, `summary`

### Shell Mode Events

`input_event_received`, `input_write_done`, `pty_output_batch`, `pty_resize`, `shell_exit`, `summary`

### Summary Fields (all modes)

Frame timing (warmup-corrected for p95/p99):
`p50/p95/p99_frame_delta_ms`, `p50/p95/p99_render_work_ms`, `p50/p95/p99_parse_time_ms`

Jank: `frames_over_16_7ms`, `frames_over_33_3ms`, `frames_over_50ms`

Queue: `output_queue_capacity_bytes`, `output_queue_policy`, `output_bytes_dropped`, `producer_block_count`, `producer_block_duration_ms`, `max_pending_pty_output_bytes`, `queue_depth_at_end_bytes`

Input (shell mode): `p50/p95/p99_input_write_latency_ms`, `max_input_write_latency_ms`, `input_events_received`, `input_events_written`, `input_events_failed`

## Smoke Tests

```bash
# Full automated smoke test (replay mode)
bash bench/smoke-test.sh

# Replay
cargo run --release --bin planeai-iced-spike -- \
  --replay bench/fixtures/mixed-agent-like.ansi \
  --cols 120 --rows 40 --chunk-size 16384 --chunk-interval-ms 4 \
  --metrics bench/results/smoke-replay.jsonl \
  --backend iced-alacritty --exit-when-done

# Command mode
cargo run --release --bin planeai-iced-spike -- \
  --command "echo hello && exit" \
  --cols 120 --rows 40 \
  --metrics bench/results/smoke-command.jsonl \
  --backend iced-alacritty --exit-when-done

# Input benchmark
cargo run --release --bin planeai-iced-spike -- \
  --command "python3 bench/flood-output.py" \
  --input-benchmark --input-interval-ms 50 --input-events 100 \
  --cols 120 --rows 40 \
  --metrics bench/results/smoke-input-latency.jsonl \
  --backend iced-alacritty --exit-when-done

# Summarize
python3 bench/summarize-metrics.py bench/results/smoke-*.jsonl
```

## Known Limitations

- **No bracketed paste mode** — paste writes raw bytes without `\e[200~`/`\e[201~` wrapping
- **No echo latency detection** — `input_echo_observed` event not implemented; echo latency fields are null
- **No bold/italic/underline** — text renders plain monospace only
- **No selection/copy** — not implemented
- **No mouse reporting** — not implemented
- **No IME support** — not implemented
- **No hyperlinks** — not implemented
- **No real GPU render_time_ms** — render timing is CPU canvas work only
- **No headless mode** — requires a window
- **No scrollback interaction** — scrollback buffer exists but no scroll UI
- **Ctrl+V on Linux** — may conflict with terminal Ctrl+V (verbatim mode); paste uses platform modifier

## Architecture

```
main.rs     — Iced app, CLI, replay loop, metrics, paste, resize
shell.rs    — PTY lifecycle (portable-pty), bounded buffer, resize, drop tracking
input.rs    — Keyboard event → terminal byte encoding (documented + tested)
```

## Summarize Results

```bash
python3 bench/summarize-metrics.py bench/results/*.jsonl
```
