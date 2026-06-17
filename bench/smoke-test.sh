#!/usr/bin/env bash
set -euo pipefail

# Smoke test for the Iced/alacritty benchmark harness integration contract.
# Verifies: fixture generation, spike execution, JSONL output, summary detection.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"
FIXTURES_DIR="$SCRIPT_DIR/fixtures"

# Resolve binary
if [[ -n "${PLANEAI_ICED_SPIKE_BIN:-}" ]]; then
  BIN="$PLANEAI_ICED_SPIKE_BIN"
else
  BIN="$REPO_ROOT/src-tauri/target/release/planeai-iced-spike"
fi

echo "=== Iced Spike Smoke Test ==="
echo "Binary: $BIN"
echo

# Step 1: Generate fixtures (including mixed-agent-like)
echo "[1/7] Generating fixtures..."
python3 "$SCRIPT_DIR/generate-fixtures.py" --size small
echo

# Step 2: Build if binary doesn't exist
if [[ ! -f "$BIN" ]]; then
  echo "[2/7] Building spike (release)..."
  (cd "$REPO_ROOT/src-tauri" && cargo build --release -p planeai-iced-spike)
else
  echo "[2/7] Binary exists, skipping build."
fi
echo

# Step 3: Run iced-alacritty benchmark on mixed-agent-like
echo "[3/7] Running iced-alacritty benchmark..."
mkdir -p "$RESULTS_DIR"
METRICS_FILE="$RESULTS_DIR/iced-alacritty_mixed-agent-like_120x40_16k_4ms_run1.jsonl"
SNAPSHOT_FILE="$RESULTS_DIR/iced-alacritty_mixed-agent-like_120x40_16k_4ms_run1.txt"

"$BIN" \
  --replay "$FIXTURES_DIR/mixed-agent-like.ansi" \
  --cols 120 \
  --rows 40 \
  --chunk-size 16384 \
  --chunk-interval-ms 4 \
  --metrics "$METRICS_FILE" \
  --backend iced-alacritty \
  --snapshot "$SNAPSHOT_FILE" \
  --exit-when-done

echo "  Metrics: $METRICS_FILE"
echo "  Snapshot: $SNAPSHOT_FILE"
echo

# Step 4: Confirm JSONL was produced
echo "[4/7] Checking JSONL output exists..."
if [[ ! -s "$METRICS_FILE" ]]; then
  echo "  FAIL: metrics file is empty or missing"
  exit 1
fi
echo "  OK: $(wc -l < "$METRICS_FILE") lines"
echo

# Step 5: Confirm last event is a summary
echo "[5/7] Checking summary event..."
LAST_EVENT_TYPE=$(python3 -c "
import json, sys
with open('$METRICS_FILE') as f:
    lines = [l.strip() for l in f if l.strip()]
obj = json.loads(lines[-1])
print(obj.get('event_type', 'MISSING'))
")
if [[ "$LAST_EVENT_TYPE" != "summary" ]]; then
  echo "  FAIL: last event_type is '$LAST_EVENT_TYPE', expected 'summary'"
  exit 1
fi
echo "  OK: event_type=summary found"
echo

# Step 6: Confirm schema_version and backend in summary
echo "[6/7] Validating summary schema..."
python3 -c "
import json, sys
with open('$METRICS_FILE') as f:
    lines = [l.strip() for l in f if l.strip()]
s = json.loads(lines[-1])
errors = []
if s.get('schema_version') != 1:
    errors.append(f'schema_version={s.get(\"schema_version\")} (expected 1)')
if s.get('backend') != 'iced-alacritty':
    errors.append(f'backend={s.get(\"backend\")} (expected iced-alacritty)')
for key in ['total_bytes','wall_time_ms','average_mb_per_sec','p50_frame_delta_ms',
            'p95_frame_delta_ms','p99_frame_delta_ms','p50_render_work_ms',
            'p50_parse_time_ms','max_queue_depth_bytes','startup_time_ms','final_rss_mb',
            'replay_mode','chunk_size','chunk_interval_ms','cols','rows','total_chunks']:
    if key not in s:
        errors.append(f'missing key: {key}')
if errors:
    print('  FAIL:')
    for e in errors:
        print(f'    - {e}')
    sys.exit(1)
print('  OK: all required summary fields present')
"
echo

# Step 7: Run summarizer
echo "[7/7] Running summarize-metrics.py..."
python3 "$SCRIPT_DIR/summarize-metrics.py" "$METRICS_FILE"
echo

echo "=== SMOKE TEST PASSED ==="
