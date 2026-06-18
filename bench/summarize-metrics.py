#!/usr/bin/env python3
"""Summarize PlaneAI benchmark results from JSONL files."""

import argparse
import json
import math
import os
import sys
from collections import defaultdict
from pathlib import Path


def read_summary(filepath):
    summary = None
    with open(filepath) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
                if event.get("event_type") == "summary":
                    summary = event
            except json.JSONDecodeError:
                continue
    return summary


def median(values):
    if not values:
        return 0
    s = sorted(values)
    n = len(s)
    return (s[n // 2 - 1] + s[n // 2]) / 2 if n % 2 == 0 else s[n // 2]


def get_parse_or_write(s, p):
    """write_latency_ms for tauri-xterm, parse_time_ms for iced-alacritty."""
    v = s.get(f"p{p}_write_latency_ms")
    if v is not None and v != 0:
        return v
    v = s.get(f"p{p}_parse_time_ms")
    if v is not None:
        return v
    # Multi-session active parse time
    v = s.get(f"p{p}_active_parse_time_ms")
    if v is not None:
        return v
    return 0


def get_frame_delta(s, p):
    v = s.get(f"p{p}_frame_delta_ms")
    if v is not None:
        return v
    return s.get(f"p{p}_frame_time_ms", 0) or 0


def get_render_work(s, p):
    v = s.get(f"p{p}_render_work_ms")
    if v:
        return v
    return s.get(f"p{p}_active_render_work_ms") or 0


def get_memory(s):
    for k in ("final_rss_mb", "final_js_heap_mb"):
        v = s.get(k)
        if v and v > 0:
            return v
    return 0


def get_max_pending_input(s):
    for k in ("max_pending_input_bytes", "max_queue_depth_bytes", "max_pending_unparsed_bytes"):
        v = s.get(k)
        if v and v > 0:
            return v
    return 0


def get_queue_end(s):
    return s.get("queue_depth_at_end_bytes", 0) or 0


def check_warnings(summaries, key):
    """Generate per-group warnings."""
    backend, fixture, cols, rows, chunk_size, interval = key
    warns = []
    n = len(summaries)

    if n == 1:
        warns.append("only 1 run")

    for s in summaries:
        # Pacing
        if interval > 0:
            wall = s.get("wall_time_ms") or s.get("total_replay_time_ms", 0)
            expected = s.get("expected_min_replay_time_ms")
            if expected is None:
                tb = s.get("total_bytes", 0)
                expected = math.ceil(tb / chunk_size) * interval if chunk_size else 0
            if expected > 0 and wall < expected * 0.8:
                warns.append(f"wall {wall:.0f}ms < expected {expected}ms")
                break

        # Queue = fixture size (not processing fast enough or preloading all)
        total_bytes = s.get("total_bytes", 0)
        pending = get_max_pending_input(s)
        if total_bytes > 0 and pending >= total_bytes * 0.95:
            warns.append("queue≈fixture_size")
            break

        # frame_delta and render_work appear mixed (render > delta)
        p95_fd = get_frame_delta(s, 95)
        p95_rw = get_render_work(s, 95)
        if p95_rw > 0 and p95_fd > 0 and p95_rw > p95_fd:
            warns.append("render>frame_delta")
            break

    return warns


def group_results(files):
    groups = defaultdict(list)
    for filepath in files:
        summary = read_summary(filepath)
        if not summary:
            print(f"  WARN: no summary in {filepath}", file=sys.stderr)
            continue
        key = (
            summary.get("backend", "unknown"),
            summary.get("fixture", Path(filepath).stem),
            summary.get("cols", 0),
            summary.get("rows", 0),
            summary.get("chunk_size", 0),
            summary.get("chunk_interval_ms", 0),
        )
        groups[key].append(summary)
    return groups


def fmt(val, decimals=2, suffix=""):
    if val is None or val == 0:
        return "-"
    return f"{val:.{decimals}f}{suffix}"


def get_mode(s):
    """Detect mode from summary."""
    m = s.get("mode")
    if m:
        return m
    if s.get("replay_mode"):
        return s.get("replay_mode")
    return "unknown"


def get_sessions(s):
    return s.get("session_count", 1)


def get_switch_latency(s, p):
    return s.get(f"p{p}_session_switch_latency_ms")


def format_table(groups):
    headers = [
        "backend", "fixture", "size", "mode", "sessions", "runs",
        "wall_ms", "MB/s",
        "p95_parse_or_write_ms", "p99_parse_or_write_ms",
        "p95_frame_delta_ms", "p99_frame_delta_ms",
        "p95_render_work_ms", "p99_render_work_ms",
        "p95_switch_latency_ms", "p99_switch_latency_ms",
        "frames>16.7", "frames>33.3", "frames>50",
        "output_bytes_dropped", "max_pending_output_KB",
        "memory_MB", "notes",
    ]

    rows = []
    for key in sorted(groups.keys()):
        summaries = groups[key]
        backend, fixture, cols, row_count, chunk_size, interval = key
        n = len(summaries)

        def med(field):
            return median([s.get(field, 0) or 0 for s in summaries])

        mode = "max" if interval == 0 else f"{interval}ms"
        fix_name = Path(fixture).stem if "/" in fixture else fixture.replace(".ansi", "")

        # Override mode for multi-session summaries
        for s in summaries:
            m = get_mode(s)
            if m == "multi-session":
                mode = "multi"
                break
            elif m not in ("unknown",):
                mode = m
                break

        warnings = check_warnings(summaries, key)

        rows.append([
            backend,
            fix_name,
            f"{cols}x{row_count}",
            mode,
            str(median([get_sessions(s) for s in summaries])),
            str(n),
            fmt(med("wall_time_ms") or med("total_replay_time_ms"), 0),
            fmt(med("average_mb_per_sec"), 2),
            fmt(median([get_parse_or_write(s, 95) for s in summaries]), 2),
            fmt(median([get_parse_or_write(s, 99) for s in summaries]), 2),
            fmt(median([get_frame_delta(s, 95) for s in summaries]), 1),
            fmt(median([get_frame_delta(s, 99) for s in summaries]), 1),
            fmt(median([get_render_work(s, 95) for s in summaries]), 2),
            fmt(median([get_render_work(s, 99) for s in summaries]), 2),
            fmt(median([get_switch_latency(s, 95) or 0 for s in summaries]), 2),
            fmt(median([get_switch_latency(s, 99) or 0 for s in summaries]), 2),
            fmt(med("frames_over_16_7ms"), 0),
            fmt(med("frames_over_33_3ms"), 0),
            fmt(med("frames_over_50ms"), 0),
            fmt(med("output_bytes_dropped_total"), 0),
            fmt(median([s.get("max_pending_pty_output_bytes_total", 0) or get_max_pending_input(s) for s in summaries]) / 1024, 0),
            fmt(median([get_memory(s) for s in summaries]), 0),
            "; ".join(warnings),
        ])

    widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))

    lines = []
    lines.append("| " + " | ".join(h.ljust(widths[i]) for i, h in enumerate(headers)) + " |")
    lines.append("| " + " | ".join("-" * widths[i] for i in range(len(headers))) + " |")
    for row in rows:
        lines.append("| " + " | ".join(row[i].ljust(widths[i]) for i in range(len(headers))) + " |")
    return "\n".join(lines)


def format_json(groups):
    results = []
    for key in sorted(groups.keys()):
        summaries = groups[key]
        backend, fixture, cols, row_count, chunk_size, interval = key

        def med(field):
            return median([s.get(field, 0) or 0 for s in summaries])

        results.append({
            "backend": backend,
            "fixture": fixture,
            "cols": cols,
            "rows": row_count,
            "chunk_size": chunk_size,
            "chunk_interval_ms": interval,
            "run_count": len(summaries),
            "median_wall_ms": med("wall_time_ms") or med("total_replay_time_ms"),
            "median_mb_per_sec": med("average_mb_per_sec"),
            "median_p95_parse_or_write_ms": median([get_parse_or_write(s, 95) for s in summaries]),
            "median_p99_parse_or_write_ms": median([get_parse_or_write(s, 99) for s in summaries]),
            "median_p95_frame_delta_ms": median([get_frame_delta(s, 95) for s in summaries]),
            "median_p99_frame_delta_ms": median([get_frame_delta(s, 99) for s in summaries]),
            "median_p95_render_work_ms": median([get_render_work(s, 95) for s in summaries]),
            "median_p99_render_work_ms": median([get_render_work(s, 99) for s in summaries]),
            "median_frames_over_16_7ms": med("frames_over_16_7ms"),
            "median_frames_over_33_3ms": med("frames_over_33_3ms"),
            "median_frames_over_50ms": med("frames_over_50ms"),
            "max_pending_input_bytes": median([get_max_pending_input(s) for s in summaries]),
            "queue_depth_at_end_bytes": median([get_queue_end(s) for s in summaries]),
            "median_memory_mb": median([get_memory(s) for s in summaries]),
            "warnings": check_warnings(summaries, key),
        })
    return json.dumps(results, indent=2)


def main():
    parser = argparse.ArgumentParser(description="Summarize PlaneAI benchmark results")
    parser.add_argument("files", nargs="+", help="JSONL result files or directories")
    parser.add_argument("--json", action="store_true", help="Output JSON instead of markdown table")
    args = parser.parse_args()

    files = []
    for f in args.files:
        if "*" in f:
            import glob
            files.extend(glob.glob(f))
        elif os.path.isfile(f):
            files.append(f)
        elif os.path.isdir(f):
            files.extend(str(p) for p in Path(f).glob("*.jsonl"))

    if not files:
        print("No JSONL files found.", file=sys.stderr)
        sys.exit(1)

    groups = group_results(files)
    if not groups:
        print("No summary events found in any file.", file=sys.stderr)
        sys.exit(1)

    if args.json:
        print(format_json(groups))
    else:
        print(format_table(groups))


if __name__ == "__main__":
    main()
