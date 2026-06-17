# PlaneAI Iced Terminal Spike

Native Rust terminal renderer using **Iced 0.14** + **alacritty_terminal 0.26**.
Supports replay benchmarking and live interactive shell mode.

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
  --cols 120 \
  --rows 40 \
  --chunk-size 16384 \
  --chunk-interval-ms 4 \
  --metrics bench/results/iced-alacritty_mixed-agent-like_120x40_16k_4ms.jsonl \
  --backend iced-alacritty \
  --exit-when-done
```

### Live Shell Mode

Opens a real PTY with your `$SHELL`. Interactive terminal with keyboard input.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell \
  --cols 120 \
  --rows 40 \
  --metrics bench/results/iced-alacritty_shell_120x40.jsonl \
  --backend iced-alacritty
```

### Command Mode

Runs a specific command inside the PTY.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --command "bash -lc 'for i in {1..10000}; do echo line-\$i; done; exec bash'" \
  --cols 120 \
  --rows 40 \
  --metrics bench/results/iced-alacritty_command_120x40.jsonl \
  --backend iced-alacritty
```

### Input Benchmark Mode

Injects synthetic keystrokes at intervals while shell output is streaming.
Measures input write latency under load.

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell \
  --input-benchmark \
  --input-interval-ms 50 \
  --input-events 100 \
  --cols 120 \
  --rows 40 \
  --metrics bench/results/iced-alacritty_input-latency_120x40.jsonl \
  --backend iced-alacritty \
  --exit-when-done
```

## Flood/Responsiveness Test (Manual)

Test typing responsiveness while output is flooding:

```bash
cargo run --release --bin planeai-iced-spike -- \
  --shell \
  --cols 120 \
  --rows 40 \
  --metrics bench/results/iced-alacritty_flood_120x40.jsonl \
  --backend iced-alacritty
```

Then inside the shell:

```bash
# Start a flood
yes "PlaneAI terminal performance test line with colors and enough text to wrap"

# Press Ctrl-C to stop
# Verify typing still works: type 'echo hello' and press Enter
```

Or with color output:

```bash
python3 -c "
import sys
colors = [31, 32, 33, 34, 35, 36]
i = 0
while True:
    c = colors[i % len(colors)]
    print(f'\033[{c}mline {i:08d} PlaneAI terminal flood\033[0m')
    i += 1
"
```

**Expected behavior:**
- Ctrl-C interrupts the flood
- Typing remains responsive during output
- UI does not freeze
- PTY output buffer stays bounded (512KB max)

## CLI Flags

| Flag | Description |
|------|-------------|
| `--replay <path>` | Raw ANSI fixture file for replay mode |
| `--shell` | Launch interactive shell |
| `--command <cmd>` | Run specific command in PTY |
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
| `--input-benchmark` | Enable synthetic input injection |
| `--input-interval-ms <ms>` | Interval between synthetic inputs (default: 50) |
| `--input-events <n>` | Number of synthetic inputs (default: 100) |

## Input Support

### Working

- Printable characters (typing)
- Enter
- Backspace
- Tab
- Escape
- Ctrl-C, Ctrl-D, Ctrl-L, Ctrl+any letter
- Arrow keys (Up/Down/Left/Right)
- Home / End
- PageUp / PageDown
- Delete / Insert

### Known Limitations

- **No paste support** — clipboard integration not yet wired
- **No resize propagation** — PTY resize is implemented but window resize events aren't captured from Iced yet
- **No bold/italic/underline** — text renders plain
- **No selection/copy** — not implemented
- **No mouse reporting** — not implemented
- **No bracketed paste mode** — not implemented
- **No IME support** — not implemented
- **No hyperlinks** — not implemented

## Metrics Emitted

### Replay Mode Events

`replay_start`, `chunk_sent`, `parse_batch`, `render_frame`, `frame_sample`, `backlog_sample`, `replay_done`, `summary`

### Shell Mode Events

`input_event_received`, `input_write_done`, `pty_output_batch`, `shell_exit`, `summary`

### Summary Fields (all modes)

Frame timing: `p50/p95/p99_frame_delta_ms`, `p50/p95/p99_render_work_ms`, `p50/p95/p99_parse_time_ms`

Jank: `frames_over_16_7ms`, `frames_over_33_3ms`, `frames_over_50ms`

Queue: `max_pending_unparsed_bytes`, `max_pending_pty_output_bytes`, `queue_depth_at_end_bytes`, `fixture_bytes_loaded`

Input (shell mode): `p50/p95/p99_input_write_latency_ms`, `input_events_received`, `input_events_written`

## Summarize Results

```bash
python3 bench/summarize-metrics.py bench/results/smoke-replay.jsonl
```

## Compare Against Tauri+xterm

```bash
# Run both backends on same fixture
python3 bench/summarize-metrics.py bench/results/iced-alacritty_*.jsonl bench/results/tauri-xterm_*.jsonl
```

## Architecture

```
main.rs     — Iced app, CLI, replay loop, metrics
shell.rs    — PTY lifecycle (portable-pty), bounded buffer, resize
input.rs    — Keyboard event → terminal byte encoding
```

PTY output is read on a background thread into a bounded 512KB buffer.
The Iced UI polls every 16ms, drains the buffer, parses through alacritty_terminal, and re-renders.
Input is encoded and written synchronously to the PTY writer.
