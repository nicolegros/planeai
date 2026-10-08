# PlaneAI Terminal Benchmark Harness

Compares the Tauri + xterm.js terminal path against a native Rust/Iced + alacritty_terminal spike. Both consume the same `.ansi` byte fixtures under identical replay settings and produce comparable JSONL metrics.

**See [INTEGRATION_CONTRACT.md](./INTEGRATION_CONTRACT.md)** for the shared schema, CLI interface, and metric naming rules.

## Quick Start

```bash
# 1. Generate fixtures
python bench/generate-fixtures.py --size medium --output-dir bench/fixtures

# 2. Build the app
make build

# 3. Run one benchmark
PLANEAI_BENCH_REPLAY=$(pwd)/bench/fixtures/mixed-agent-like.ansi \
PLANEAI_BENCH_METRICS=$(pwd)/bench/results/tauri-xterm_mixed-agent-like_120x40_16k_4ms_run1.jsonl \
PLANEAI_BENCH_EXIT=1 \
./src-tauri/target/release/planeai

# 4. Summarize
python bench/summarize-metrics.py bench/results/*.jsonl
```

## Generate Synthetic Fixtures

```bash
python bench/generate-fixtures.py --size medium --output-dir bench/fixtures
```

- `--size small|medium|large` — ~256KB / ~2MB / ~16MB
- `--fixtures` — Specific fixtures (default: all five)

| Fixture                 | Content                                         |
| ----------------------- | ----------------------------------------------- |
| `ansi-flood.ansi`       | High-volume colored text                        |
| `long-lines.ansi`       | Long markdown-like lines                        |
| `colors-heavy.ansi`     | Dense ANSI color/style sequences                |
| `progress-bars.ansi`    | Carriage-return progress, spinners              |
| `mixed-agent-like.ansi` | AI agent output (code blocks, errors, thinking) |

## Capture a Real Session

```bash
PLANEAI_BENCH_CAPTURE=$(pwd)/bench/captures/my-session.ansi make dev
```

Optionally filter to one session:

```bash
PLANEAI_BENCH_CAPTURE=$(pwd)/bench/captures/real.ansi \
PLANEAI_BENCH_CAPTURE_SESSION=<uuid> \
make dev
```

## Run Tauri+xterm Benchmark

### Direct (env vars)

```bash
PLANEAI_BENCH_REPLAY=$(pwd)/bench/fixtures/mixed-agent-like.ansi \
PLANEAI_BENCH_COLS=120 \
PLANEAI_BENCH_ROWS=40 \
PLANEAI_BENCH_CHUNK_SIZE=16384 \
PLANEAI_BENCH_CHUNK_INTERVAL_MS=4 \
PLANEAI_BENCH_METRICS=$(pwd)/bench/results/tauri-xterm_mixed-agent-like_120x40_16k_4ms_run1.jsonl \
PLANEAI_BENCH_SNAPSHOT=$(pwd)/bench/results/tauri-xterm_mixed-agent-like_120x40_16k_4ms_run1.txt \
PLANEAI_BENCH_EXIT=1 \
./src-tauri/target/release/planeai
```

### Automated runner

```bash
python bench/run-benchmark.py \
  --fixtures mixed-agent-like.ansi long-lines.ansi \
  --backends tauri-xterm \
  --runs 3 --mode realtime \
  --output-dir bench/results
```

## Run Iced Spike Benchmark

Set the binary path (avoids rebuild each run):

```bash
export PLANEAI_ICED_SPIKE_BIN=./target/release/planeai-iced-spike
```

Then run:

```bash
python bench/run-benchmark.py \
  --backends tauri-xterm iced-alacritty \
  --fixtures mixed-agent-like.ansi \
  --runs 3 --mode realtime
```

Or directly:

```bash
planeai-iced-spike \
  --replay bench/fixtures/mixed-agent-like.ansi \
  --cols 120 --rows 40 --chunk-size 16384 --chunk-interval-ms 4 \
  --metrics bench/results/iced-alacritty_mixed-agent-like_120x40_16k_4ms_run1.jsonl \
  --backend iced-alacritty --exit-when-done
```

## Summarize Results

```bash
python bench/summarize-metrics.py bench/results/*.jsonl
python bench/summarize-metrics.py bench/results/*.jsonl --json
```

The summarizer handles both backend schemas automatically and shows a unified comparison table.

## Metrics Reference

See [INTEGRATION_CONTRACT.md](./INTEGRATION_CONTRACT.md) for the full schema.

Key metrics:

| Metric                  | Backend        | Meaning                       |
| ----------------------- | -------------- | ----------------------------- |
| `write_latency_ms`      | tauri-xterm    | term.write → callback         |
| `parse_time_ms`         | iced-alacritty | bytes → terminal state        |
| `frame_delta_ms`        | both           | time between visual frames    |
| `render_work_ms`        | iced-alacritty | GPU render pass time          |
| `average_mb_per_sec`    | both           | throughput                    |
| `frames_over_16_7ms`    | both           | frames exceeding 60fps budget |
| `max_queue_depth_bytes` | both           | peak backlog                  |

## Pass/Fail Guidance

**Realtime** (chunk_interval_ms > 0):

- p95 frame delta ≥30% lower than tauri-xterm, OR frames>33.3ms cut by ≥50%
- Queue depth doesn't grow unbounded

**Maxspeed** (chunk_interval_ms = 0):

- ≥1.5× higher MB/s, OR similar throughput with better p95/p99 frame time

**Memory**: Not materially worse; stops growing after replay.

**Pacing**: The harness warns if `wall_time_ms < expected_min_replay_time_ms * 0.8`.

## Result Naming

```
{backend}_{fixture_stem}_{cols}x{rows}_{chunk_kb}k_{interval_ms}ms_run{n}.jsonl
```

Examples:

```
tauri-xterm_mixed-agent-like_120x40_16k_4ms_run1.jsonl
iced-alacritty_mixed-agent-like_120x40_16k_4ms_run1.jsonl
```

## File Structure

```
bench/
├── INTEGRATION_CONTRACT.md  ← shared schema between harness and spike
├── README.md                ← this file
├── generate-fixtures.py     ← synthetic fixture generator
├── run-benchmark.py         ← automated benchmark runner
├── summarize-metrics.py     ← results summarizer
├── fixtures/                ← generated .ansi files (gitignored)
├── captures/                ← real session captures (gitignored)
└── results/                 ← JSONL metrics + snapshots (gitignored)

src/lib/benchmark/
├── metrics.ts               ← JSONL collector (tauri-xterm)
└── replay.ts                ← replay orchestrator

src-tauri/src/bench.rs       ← Rust commands for replay + metrics I/O
```

## Known Limitations

- WebGL renderer performance varies with GPU state
- `performance.memory` only works in Chromium-based webviews (js_heap_mb = null otherwise)
- First run may include initialization overhead — use ≥3 runs and look at median
- Iced spike is not implemented in this repo — only detected and invoked
