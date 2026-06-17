# Iced Terminal Rendering Spike — Benchmark Harness

Spike to evaluate native Rust terminal rendering with **Iced** + **alacritty_terminal** as an alternative to the current Tauri + xterm.js path.

## Quick Start

```bash
# 1. Generate fixture files (~2 MB each)
python3 bench/generate-fixtures.py

# 2. Build the spike in release mode
cargo build --release -p planeai-iced-spike

# 3. Run the replay benchmark
cargo run --release --bin planeai-iced-spike -- \
  --replay bench/fixtures/ansi-flood.ansi \
  --cols 120 \
  --rows 40 \
  --chunk-size 16384 \
  --chunk-interval-ms 4 \
  --metrics bench/results/iced-ansi-flood.jsonl \
  --exit-when-done

# 4. Summarize results
python3 bench/summarize-metrics.py bench/results/*.jsonl
```

## Building

From the repo root:

```bash
cd src-tauri
cargo build --release -p planeai-iced-spike
```

The binary is at `target/release/planeai-iced-spike`.

## Generating Fixtures

```bash
python3 bench/generate-fixtures.py [size_in_bytes]
```

Default size is 2 MB per fixture. Generates:

| Fixture | Description |
|---------|-------------|
| `ansi-flood.ansi` | High-throughput colored text lines |
| `long-lines.ansi` | Lines exceeding terminal width (wrap testing) |
| `colors-heavy.ansi` | Dense 256-color and truecolor sequences |
| `progress-bars.ansi` | Carriage-return progress bars (overwrite testing) |

## Running the Benchmark

```bash
cargo run --release --bin planeai-iced-spike -- [OPTIONS]
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--replay <path>` | required | Path to `.ansi` fixture file |
| `--cols <n>` | 120 | Terminal columns |
| `--rows <n>` | 40 | Terminal rows |
| `--chunk-size <bytes>` | 16384 | Bytes fed per tick |
| `--chunk-interval-ms <ms>` | 4 | Milliseconds between chunks |
| `--metrics <path>` | none | Output JSONL metrics file |
| `--exit-when-done` | false | Exit after replay completes |

## Metrics Output

The `--metrics` file is JSONL. Each line except the last is a per-chunk event:

```json
{"timestamp_ms":12,"event_type":"chunk","bytes_total":16384,"bytes_since_last_event":16384,"frames_total":1,"parse_time_ms":0.42,"render_time_ms":0.0,"frame_time_ms":0.55,"dirty_rows":40,"queue_depth_bytes":2080768,"rss_mb":45.2,"cols":120,"rows":40}
```

The last line is the summary:

```json
{"total_bytes":2097152,"total_replay_time_ms":520.3,"average_mb_per_sec":3.84,"p50_frame_time_ms":0.35,"p95_frame_time_ms":0.82,"p99_frame_time_ms":1.4,"frames_over_16_7ms":0,"frames_over_33_3ms":0,"p50_parse_time_ms":0.30,"p95_parse_time_ms":0.70,"p99_parse_time_ms":1.1,"max_queue_depth_bytes":2080768,"final_rss_mb":48.0}
```

## Summarizing Results

```bash
python3 bench/summarize-metrics.py bench/results/*.jsonl
```

Prints a comparison table across all metric files.

## Comparing Against Tauri + xterm.js

To compare, run the same fixture through the existing xterm.js renderer (via the Tauri app or a standalone xterm.js harness), then compare:

1. **Throughput (MB/s)** — how fast bytes are consumed
2. **Frame time percentiles** — smoothness of rendering
3. **Parse time percentiles** — how fast the terminal parser processes input
4. **RSS** — memory overhead

The Iced spike is headless-friendly (with `--exit-when-done`) and produces structured metrics, making automated comparison easy.

## What's Implemented

- [x] Replay mode: reads fixture, feeds to alacritty_terminal in chunks
- [x] Configurable chunk size and interval
- [x] Canvas-based grid rendering with monospace font
- [x] ANSI foreground/background colors (16 named + 256 indexed + truecolor)
- [x] Cursor rendering
- [x] JSONL metrics with per-chunk events and final summary
- [x] Percentile calculations (p50/p95/p99)
- [x] RSS measurement (macOS)
- [x] Fixture generator (4 types)
- [x] Metrics summarizer script

## What's Missing / Known Issues

- [ ] Bold/italic/underline text attributes not rendered
- [ ] Selection support
- [ ] Hyperlink rendering
- [ ] Mouse interaction
- [ ] Live PTY mode (stretch goal)
- [ ] render_time_ms is approximate (measured as frame_time - parse_time, not actual GPU time)
- [ ] Font metrics are approximated (fixed cell width/height based on window size)
- [ ] No scrollback rendering (only visible viewport — this is intentional)
- [ ] Terminal resize during replay not supported

## Architecture

```
┌─────────────────────────────────────────────┐
│  main.rs                                     │
│                                              │
│  ┌─────────┐   chunks    ┌───────────────┐  │
│  │ Fixture │ ──────────▶ │ alacritty_    │  │
│  │  file   │             │ terminal::Term │  │
│  └─────────┘             └───────┬───────┘  │
│                                  │ grid()   │
│                          ┌───────▼───────┐  │
│                          │ GridSnapshot   │  │
│                          └───────┬───────┘  │
│                                  │          │
│                          ┌───────▼───────┐  │
│                          │ Iced Canvas   │  │
│                          │ (TermRenderer) │  │
│                          └───────────────┘  │
│                                              │
│  Metrics ──▶ JSONL file                      │
└─────────────────────────────────────────────┘
```

## File Layout

```
bench/
├── README.md              ← this file
├── generate-fixtures.py   ← fixture generator
├── summarize-metrics.py   ← metrics comparison tool
├── fixtures/              ← generated .ansi files (gitignored)
└── results/               ← output .jsonl files (gitignored)
src-tauri/
└── planeai-iced-spike/
    ├── Cargo.toml
    └── src/main.rs        ← the spike binary
```
