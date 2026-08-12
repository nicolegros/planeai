#!/usr/bin/env bash
# Run tauri-xterm benchmarks: full matrix, 3 runs each.
# Requires: make build (already done)
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="./src-tauri/target/release/planeai"
RESULTS="bench/results"
mkdir -p "$RESULTS"

run_bench() {
  local fixture="$1" cols="$2" rows="$3" interval="$4" run="$5"
  local stem=$(basename "$fixture" .ansi)
  local mode_label="${interval}ms"
  [ "$interval" = "0" ] && mode_label="max"
  local name="tauri-xterm_${stem}_${cols}x${rows}_16k_${mode_label}_run${run}"
  local metrics="${RESULTS}/${name}.jsonl"
  local snapshot="${RESULTS}/${name}.txt"

  echo "  [tauri-xterm] run${run}: ${stem} ${cols}x${rows} ${mode_label}"
  rm -f "$metrics"

  PLANEAI_BENCH_REPLAY="$(pwd)/$fixture" \
  PLANEAI_BENCH_COLS="$cols" \
  PLANEAI_BENCH_ROWS="$rows" \
  PLANEAI_BENCH_CHUNK_SIZE=16384 \
  PLANEAI_BENCH_CHUNK_INTERVAL_MS="$interval" \
  PLANEAI_BENCH_METRICS="$(pwd)/$metrics" \
  PLANEAI_BENCH_SNAPSHOT="$(pwd)/$snapshot" \
  PLANEAI_BENCH_EXIT=1 \
  "$BIN" 2>/dev/null || true

  [ -f "$metrics" ] && echo "    ✓ $metrics" || echo "    ✗ no metrics"
  sleep 0.5
}

echo "=== Tauri+xterm benchmark matrix ==="
echo ""

# 1. mixed-agent-like 120x40 4ms
for r in 1 2 3; do run_bench bench/fixtures/mixed-agent-like.ansi 120 40 4 $r; done

# 2. mixed-agent-like 120x40 max
for r in 1 2 3; do run_bench bench/fixtures/mixed-agent-like.ansi 120 40 0 $r; done

# 3. mixed-agent-like 200x60 4ms
for r in 1 2 3; do run_bench bench/fixtures/mixed-agent-like.ansi 200 60 4 $r; done

# 4. colors-heavy 120x40 4ms
for r in 1 2 3; do run_bench bench/fixtures/colors-heavy.ansi 120 40 4 $r; done

# 5. long-lines 120x40 4ms
for r in 1 2 3; do run_bench bench/fixtures/long-lines.ansi 120 40 4 $r; done

# 6. progress-bars 120x40 4ms
for r in 1 2 3; do run_bench bench/fixtures/progress-bars.ansi 120 40 4 $r; done

# 7. agent-real-slow 120x40 4ms
for r in 1 2 3; do run_bench bench/captures/agent-real-slow.ansi 120 40 4 $r; done

# 8. agent-real-slow 120x40 max
for r in 1 2 3; do run_bench bench/captures/agent-real-slow.ansi 120 40 0 $r; done

# 9. agent-real-slow 200x60 4ms
for r in 1 2 3; do run_bench bench/captures/agent-real-slow.ansi 200 60 4 $r; done

echo ""
echo "=== Done. Summarize with: ==="
echo "  uv run bench/summarize-metrics.py bench/results/*.jsonl"
