#!/usr/bin/env python3
"""Run PlaneAI terminal benchmark suite.

Runs benchmarks for the Tauri+xterm path and optionally the Iced spike.
Conforms to bench/INTEGRATION_CONTRACT.md.
"""

import argparse
import math
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_FIXTURES = ["mixed-agent-like.ansi", "long-lines.ansi"]
DEFAULT_COLS = 120
DEFAULT_ROWS = 40
DEFAULT_CHUNK_SIZE = 16384
DEFAULT_CHUNK_INTERVAL_REALTIME = 4
DEFAULT_CHUNK_INTERVAL_MAXSPEED = 0
DEFAULT_RUNS = 3

# ─── Binary detection ─────────────────────────────────────────────────────────


def find_iced_spike():
    """Find Iced spike binary. Prefers PLANEAI_ICED_SPIKE_BIN env var."""
    env_bin = os.environ.get("PLANEAI_ICED_SPIKE_BIN")
    if env_bin:
        if os.path.isfile(env_bin) and os.access(env_bin, os.X_OK):
            return os.path.abspath(env_bin)
        print(f"WARN: PLANEAI_ICED_SPIKE_BIN={env_bin} not found or not executable")

    candidates = [
        "planeai-iced-spike",
        "target/release/planeai-iced-spike",
        "src-tauri/target/release/planeai-iced-spike",
    ]
    for name in candidates:
        path = shutil.which(name)
        if path:
            return path
        if os.path.isfile(name) and os.access(name, os.X_OK):
            return os.path.abspath(name)
    return None


def find_tauri_binary():
    """Find the PlaneAI Tauri binary."""
    candidates = [
        "src-tauri/target/release/planeai",
        "target/release/planeai",
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return os.path.abspath(c)
    path = shutil.which("planeai")
    return path


# ─── Result naming ────────────────────────────────────────────────────────────


def result_filename(backend, fixture, cols, rows, chunk_size, interval_ms, run):
    """Generate result filename per contract naming convention."""
    fixture_stem = Path(fixture).stem
    chunk_kb = f"{chunk_size // 1024}k" if chunk_size >= 1024 else str(chunk_size)
    return f"{backend}_{fixture_stem}_{cols}x{rows}_{chunk_kb}_{interval_ms}ms_run{run}.jsonl"


# ─── Benchmark runners ────────────────────────────────────────────────────────


def run_tauri_bench(binary, fixture_path, cols, rows, chunk_size, interval_ms, output_dir, run):
    """Run a single Tauri+xterm benchmark."""
    fixture_abs = os.path.abspath(fixture_path)
    metrics_file = result_filename("tauri-xterm", fixture_path, cols, rows, chunk_size, interval_ms, run)
    metrics_path = os.path.abspath(os.path.join(output_dir, metrics_file))
    snapshot_path = metrics_path.replace(".jsonl", ".txt")

    if os.path.exists(metrics_path):
        os.remove(metrics_path)

    env = {
        **os.environ,
        "PLANEAI_BENCH_REPLAY": fixture_abs,
        "PLANEAI_BENCH_COLS": str(cols),
        "PLANEAI_BENCH_ROWS": str(rows),
        "PLANEAI_BENCH_CHUNK_SIZE": str(chunk_size),
        "PLANEAI_BENCH_CHUNK_INTERVAL_MS": str(interval_ms),
        "PLANEAI_BENCH_METRICS": metrics_path,
        "PLANEAI_BENCH_SNAPSHOT": snapshot_path,
        "PLANEAI_BENCH_EXIT": "1",
    }

    print(f"  [{_label('tauri-xterm')}] run {run}: {Path(fixture_path).stem} "
          f"({cols}x{rows}, {chunk_size}B, {interval_ms}ms)")

    try:
        subprocess.run([binary], env=env, timeout=120, capture_output=True)
    except subprocess.TimeoutExpired:
        print("    WARN: timed out after 120s")
        return None
    except FileNotFoundError:
        print(f"    ERROR: binary not found: {binary}")
        return None

    return metrics_path if os.path.exists(metrics_path) else None


def run_iced_bench(binary, fixture_path, cols, rows, chunk_size, interval_ms, output_dir, run):
    """Run a single Iced spike benchmark per integration contract."""
    fixture_abs = os.path.abspath(fixture_path)
    metrics_file = result_filename("iced-alacritty", fixture_path, cols, rows, chunk_size, interval_ms, run)
    metrics_path = os.path.abspath(os.path.join(output_dir, metrics_file))
    snapshot_path = metrics_path.replace(".jsonl", ".txt")

    if os.path.exists(metrics_path):
        os.remove(metrics_path)

    print(f"  [{_label('iced-alacritty')}] run {run}: {Path(fixture_path).stem} "
          f"({cols}x{rows}, {chunk_size}B, {interval_ms}ms)")

    cmd = [
        binary,
        "--replay", fixture_abs,
        "--cols", str(cols),
        "--rows", str(rows),
        "--chunk-size", str(chunk_size),
        "--chunk-interval-ms", str(interval_ms),
        "--metrics", metrics_path,
        "--backend", "iced-alacritty",
        "--snapshot", snapshot_path,
        "--exit-when-done",
    ]

    try:
        subprocess.run(cmd, timeout=120, capture_output=True)
    except subprocess.TimeoutExpired:
        print("    WARN: timed out")
        return None
    except FileNotFoundError:
        print(f"    ERROR: binary not found: {binary}")
        return None

    return metrics_path if os.path.exists(metrics_path) else None


def _label(name):
    colors = {"tauri-xterm": "\033[36m", "iced-alacritty": "\033[33m"}
    return f"{colors.get(name, '')}{name}\033[0m"


# ─── Pacing check ────────────────────────────────────────────────────────────


def check_pacing(metrics_path, chunk_size, interval_ms):
    """Warn if wall_time is suspiciously low for realtime mode."""
    if interval_ms == 0:
        return
    import json
    try:
        with open(metrics_path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                ev = json.loads(line)
                if ev.get("event_type") == "summary":
                    total_bytes = ev.get("total_bytes", 0)
                    wall_time = ev.get("wall_time_ms") or ev.get("total_replay_time_ms", 0)
                    expected = math.ceil(total_bytes / chunk_size) * interval_ms
                    if expected > 0 and wall_time < expected * 0.8:
                        print(f"    ⚠ PACING WARNING: wall_time={wall_time:.0f}ms < "
                              f"expected_min={expected}ms (replay may not be pacing correctly)")
                    break
    except (json.JSONDecodeError, OSError):
        pass


# ─── Main ─────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description="Run PlaneAI terminal benchmarks")
    parser.add_argument("--fixtures", nargs="+", default=DEFAULT_FIXTURES)
    parser.add_argument("--backends", nargs="+", default=["tauri-xterm"],
                        choices=["tauri-xterm", "iced-alacritty"])
    parser.add_argument("--runs", type=int, default=DEFAULT_RUNS)
    parser.add_argument("--cols", type=int, default=DEFAULT_COLS)
    parser.add_argument("--rows", type=int, default=DEFAULT_ROWS)
    parser.add_argument("--chunk-size", type=int, default=DEFAULT_CHUNK_SIZE)
    parser.add_argument("--chunk-interval-ms", type=int, default=None)
    parser.add_argument("--mode", choices=["realtime", "maxspeed"], default="realtime")
    parser.add_argument("--output-dir", default="bench/results")
    parser.add_argument("--fixture-dir", default="bench/fixtures")
    parser.add_argument("--tauri-binary", default=None)
    parser.add_argument("--iced-binary", default=None, help="Path to iced spike binary (overrides PLANEAI_ICED_SPIKE_BIN)")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    interval = (args.chunk_interval_ms if args.chunk_interval_ms is not None
                else DEFAULT_CHUNK_INTERVAL_MAXSPEED if args.mode == "maxspeed"
                else DEFAULT_CHUNK_INTERVAL_REALTIME)

    os.makedirs(args.output_dir, exist_ok=True)

    # Resolve binaries
    tauri_bin = args.tauri_binary or find_tauri_binary()
    iced_bin = args.iced_binary or (find_iced_spike() if "iced-alacritty" in args.backends else None)

    if "tauri-xterm" in args.backends and not tauri_bin:
        print("ERROR: PlaneAI binary not found. Build with: pnpm tauri build")
        print("       Or specify with --tauri-binary")
        sys.exit(1)

    if "iced-alacritty" in args.backends and not iced_bin:
        print("WARN: Iced spike binary not found (set PLANEAI_ICED_SPIKE_BIN or --iced-binary)")
        args.backends = [b for b in args.backends if b != "iced-alacritty"]

    # Resolve fixtures
    fixtures = []
    for f in args.fixtures:
        if os.path.isfile(f):
            fixtures.append(f)
        else:
            path = os.path.join(args.fixture_dir, f)
            if os.path.isfile(path):
                fixtures.append(path)
            else:
                print(f"WARN: fixture not found: {f}")

    if not fixtures:
        print("ERROR: no fixtures found. Run: python bench/generate-fixtures.py")
        sys.exit(1)

    print(f"Benchmark: {len(fixtures)} fixtures × {len(args.backends)} backends × {args.runs} runs")
    print(f"  Mode: {args.mode} (interval={interval}ms, chunk={args.chunk_size}B)")
    print(f"  Terminal: {args.cols}x{args.rows}")
    print()

    results = []

    for fixture in fixtures:
        for backend in args.backends:
            for run in range(1, args.runs + 1):
                if args.dry_run:
                    print(f"  [dry-run] {backend} {Path(fixture).stem} run{run}")
                    continue

                if backend == "tauri-xterm":
                    r = run_tauri_bench(tauri_bin, fixture, args.cols, args.rows, args.chunk_size, interval, args.output_dir, run)
                elif backend == "iced-alacritty":
                    r = run_iced_bench(iced_bin, fixture, args.cols, args.rows, args.chunk_size, interval, args.output_dir, run)
                else:
                    continue

                if r:
                    results.append(r)
                    check_pacing(r, args.chunk_size, interval)

                time.sleep(0.5)

    print()
    if results:
        print(f"Done. {len(results)} result files in {args.output_dir}/")
        print(f"\nSummarize with:\n  python bench/summarize-metrics.py {args.output_dir}/*.jsonl")
    elif args.dry_run:
        print("(dry run — no benchmarks executed)")


if __name__ == "__main__":
    main()
