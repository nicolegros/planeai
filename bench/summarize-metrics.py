#!/usr/bin/env python3
"""Summarize JSONL metric files from iced-alacritty and/or tauri-xterm benchmarks.

Reads the summary event (event_type: "summary") from each JSONL file and prints
a comparison table. Works with both backends' output.
"""

import json
import sys


def load_summary(path: str) -> dict | None:
    """Find the summary event in a JSONL file (line with event_type == 'summary')."""
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get("event_type") == "summary":
                obj["_file"] = path
                return obj
    return None


def fmt_bytes(n) -> str:
    if n is None:
        return "—"
    n = int(n)
    if n >= 1_048_576:
        return f"{n / 1_048_576:.1f} MB"
    if n >= 1024:
        return f"{n / 1024:.1f} KB"
    return f"{n} B"


def fmt_f(v, decimals=2) -> str:
    if v is None:
        return "—"
    return f"{v:.{decimals}f}"


def print_table(summaries: list[dict]):
    headers = [
        "File",
        "Backend",
        "Mode",
        "Cols×Rows",
        "Chunk",
        "Intrvl",
        "Chunks",
        "Bytes",
        "Wall ms",
        "MB/s",
        "p50 Δfrm",
        "p95 Δfrm",
        "p99 Δfrm",
        "p50 rnd",
        "p95 rnd",
        "p99 rnd",
        "p50 prs",
        "p95 prs",
        "p99 prs",
        ">16.7",
        ">33.3",
        ">50",
        "MaxQ",
        "Startup",
        "RSS",
    ]

    rows = []
    for s in summaries:
        fname = s.get("_file", "?")
        if "/" in fname:
            fname = fname.rsplit("/", 1)[-1]
        rows.append([
            fname,
            s.get("backend", "?"),
            s.get("replay_mode", "?"),
            f"{s.get('cols', '?')}×{s.get('rows', '?')}",
            fmt_bytes(s.get("chunk_size")),
            f"{s.get('chunk_interval_ms', '?')}ms",
            str(s.get("total_chunks", s.get("actual_chunk_count", "?"))),
            fmt_bytes(s.get("total_bytes")),
            fmt_f(s.get("wall_time_ms", s.get("total_replay_time_ms")), 0) + "ms",
            fmt_f(s.get("average_mb_per_sec")),
            fmt_f(s.get("p50_frame_delta_ms")),
            fmt_f(s.get("p95_frame_delta_ms")),
            fmt_f(s.get("p99_frame_delta_ms")),
            fmt_f(s.get("p50_render_work_ms")),
            fmt_f(s.get("p95_render_work_ms")),
            fmt_f(s.get("p99_render_work_ms")),
            fmt_f(s.get("p50_parse_time_ms")),
            fmt_f(s.get("p95_parse_time_ms")),
            fmt_f(s.get("p99_parse_time_ms")),
            str(s.get("frames_over_16_7ms", "—")),
            str(s.get("frames_over_33_3ms", "—")),
            str(s.get("frames_over_50ms", "—")),
            fmt_bytes(s.get("max_queue_depth_bytes", s.get("max_pending_unparsed_bytes"))),
            fmt_f(s.get("startup_time_ms"), 1) + "ms",
            fmt_f(s.get("final_rss_mb"), 1),
        ])

    widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))

    def fmt_row(cells):
        return " | ".join(c.ljust(widths[i]) for i, c in enumerate(cells))

    print(fmt_row(headers))
    print("-+-".join("-" * w for w in widths))
    for row in rows:
        print(fmt_row(row))

    # Pacing warnings
    print()
    for s in summaries:
        expected = s.get("expected_min_replay_time_ms")
        wall = s.get("wall_time_ms", s.get("total_replay_time_ms"))
        if expected and wall and s.get("replay_mode") == "realtime":
            if wall < expected * 0.8:
                fname = s.get("_file", "?").rsplit("/", 1)[-1]
                print(f"⚠ PACING WARNING: {fname} wall_time={wall:.0f}ms < expected_min={expected}ms")


def main():
    if len(sys.argv) < 2:
        print("Usage: summarize-metrics.py <file1.jsonl> [file2.jsonl ...]")
        sys.exit(1)

    summaries = []
    for path in sys.argv[1:]:
        s = load_summary(path)
        if s:
            summaries.append(s)
        else:
            print(f"Warning: no summary event found in {path}", file=sys.stderr)

    if summaries:
        print_table(summaries)
    else:
        print("No summaries found.")
        sys.exit(1)


if __name__ == "__main__":
    main()
