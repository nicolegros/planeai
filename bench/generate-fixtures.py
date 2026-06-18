#!/usr/bin/env python3
"""Generate deterministic synthetic ANSI fixtures for PlaneAI benchmarks."""

import argparse
import os
import random

SIZES = {"small": 256_000, "medium": 2_000_000, "large": 16_000_000}

# ANSI escape helpers
RESET = "\033[0m"
COLORS = [f"\033[{c}m" for c in range(31, 38)]
BOLD = "\033[1m"
DIM = "\033[2m"
UNDERLINE = "\033[4m"
BG_COLORS = [f"\033[{c}m" for c in range(41, 48)]
COLOR_256 = [f"\033[38;5;{i}m" for i in range(16, 232)]


def make_rng(seed=42):
    return random.Random(seed)


def generate_ansi_flood(target_bytes, rng):
    """Pure colored text flood."""
    lines = []
    size = 0
    words = ["error", "warning", "info", "debug", "trace", "compiling", "linking", "done"]
    while size < target_bytes:
        color = rng.choice(COLORS)
        word = rng.choice(words)
        line = f"{color}{word}{RESET} " * rng.randint(5, 20) + "\r\n"
        lines.append(line)
        size += len(line)
    return "".join(lines)


def generate_long_lines(target_bytes, rng):
    """Long markdown-like lines with occasional wrapping."""
    lines = []
    size = 0
    md_prefixes = ["# ", "## ", "### ", "- ", "> ", "  - ", "```\n", ""]
    words = "the quick brown fox jumps over the lazy dog while compiling rust code with many dependencies".split()
    while size < target_bytes:
        prefix = rng.choice(md_prefixes)
        length = rng.randint(80, 400)
        content = " ".join(rng.choice(words) for _ in range(length // 5))
        line = f"{prefix}{content}\r\n"
        lines.append(line)
        size += len(line)
    return "".join(lines)


def generate_colors_heavy(target_bytes, rng):
    """Heavy color/style usage typical of build tools."""
    lines = []
    size = 0
    while size < target_bytes:
        parts = []
        for _ in range(rng.randint(3, 15)):
            fg = rng.choice(COLOR_256)
            bg = rng.choice(BG_COLORS) if rng.random() < 0.3 else ""
            style = rng.choice([BOLD, DIM, UNDERLINE, ""]) 
            text = rng.choice(["PASS", "FAIL", "WARN", "src/", "test/", "→", "✓", "✗", "●"])
            parts.append(f"{style}{fg}{bg}{text}{RESET}")
        line = " ".join(parts) + "\r\n"
        lines.append(line)
        size += len(line)
    return "".join(lines)


def generate_progress_bars(target_bytes, rng):
    """Carriage-return progress updates, spinners, status lines."""
    lines = []
    size = 0
    spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    tasks = ["Compiling", "Downloading", "Installing", "Building", "Testing", "Uploading"]
    while size < target_bytes:
        task = rng.choice(tasks)
        # Simulate progress bar updates with CR
        for pct in range(0, 101, rng.randint(1, 5)):
            spinner = spinners[pct % len(spinners)]
            bar_len = 40
            filled = int(bar_len * pct / 100)
            bar = "█" * filled + "░" * (bar_len - filled)
            line = f"\r{COLORS[2]}{spinner}{RESET} {task}... [{bar}] {pct}%"
            lines.append(line)
            size += len(line.encode())
            if size >= target_bytes:
                break
        lines.append(f"\r\n{COLORS[1]}✓{RESET} {task} complete\r\n")
        size += 30
    return "".join(lines)


def generate_mixed_agent_like(target_bytes, rng):
    """Simulates real AI agent output: thinking, code blocks, status, URLs."""
    lines = []
    size = 0
    thinking = [
        "I'll analyze the codebase structure...",
        "Let me look at the relevant files...",
        "Based on the error, I think we need to...",
        "Here's my plan:",
        "Step 1: Modify the configuration",
        "Step 2: Update the handler",
        "Step 3: Add tests",
    ]
    code_langs = ["rust", "typescript", "python", "bash"]
    urls = [
        "https://docs.rs/tokio/latest/tokio/",
        "https://github.com/nicol egros/planeai/issues/42",
        "https://stackoverflow.com/questions/12345",
    ]

    while size < target_bytes:
        block_type = rng.choices(
            ["thinking", "code", "status", "error", "url", "blank"],
            weights=[30, 35, 15, 10, 5, 5],
        )[0]

        if block_type == "thinking":
            for _ in range(rng.randint(1, 4)):
                line = f"{DIM}{rng.choice(thinking)}{RESET}\r\n"
                lines.append(line)
                size += len(line)
        elif block_type == "code":
            lang = rng.choice(code_langs)
            lines.append(f"\r\n{BOLD}```{lang}{RESET}\r\n")
            for _ in range(rng.randint(5, 30)):
                indent = "    " * rng.randint(0, 3)
                code_line = f"{indent}{rng.choice(COLORS)}fn {rng.choice(['process', 'handle', 'parse', 'render'])}(){RESET} {{\r\n"
                lines.append(code_line)
                size += len(code_line)
            lines.append(f"{BOLD}```{RESET}\r\n\r\n")
            size += 10
        elif block_type == "status":
            # Bursty status with spinner
            for i in range(rng.randint(3, 10)):
                spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[i % 10]
                line = f"\r{COLORS[3]}{spinner}{RESET} Processing file {i+1}/10..."
                lines.append(line)
                size += len(line)
            lines.append(f"\r{COLORS[1]}✓{RESET} Done processing files\r\n")
            size += 30
        elif block_type == "error":
            line = f"{COLORS[0]}{BOLD}error[E0308]{RESET}: mismatched types\r\n"
            lines.append(line)
            lines.append(f"  {COLORS[3]}-->{RESET} src/main.rs:42:5\r\n")
            lines.append(f"   {DIM}|{RESET}\r\n")
            lines.append(f"42 {DIM}|{RESET}     let x: u32 = \"hello\";\r\n")
            lines.append(f"   {DIM}|{RESET}                  {COLORS[0]}^^^^^^^ expected `u32`, found `&str`{RESET}\r\n\r\n")
            size += 200
        elif block_type == "url":
            url = rng.choice(urls)
            line = f"  See: {UNDERLINE}{COLORS[4]}{url}{RESET}\r\n"
            lines.append(line)
            size += len(line)
        else:
            lines.append("\r\n")
            size += 2

    return "".join(lines)


GENERATORS = {
    "ansi-flood": generate_ansi_flood,
    "long-lines": generate_long_lines,
    "colors-heavy": generate_colors_heavy,
    "progress-bars": generate_progress_bars,
    "mixed-agent-like": generate_mixed_agent_like,
}


def main():
    parser = argparse.ArgumentParser(description="Generate synthetic ANSI benchmark fixtures")
    parser.add_argument("--size", choices=SIZES.keys(), default="medium", help="Target file size")
    parser.add_argument("--output-dir", default="bench/fixtures", help="Output directory")
    parser.add_argument("--fixtures", nargs="*", default=list(GENERATORS.keys()), help="Which fixtures to generate")
    args = parser.parse_args()

    target = SIZES[args.size]
    os.makedirs(args.output_dir, exist_ok=True)

    for name in args.fixtures:
        if name not in GENERATORS:
            print(f"Unknown fixture: {name}")
            continue
        rng = make_rng(seed=42)
        content = GENERATORS[name](target, rng)
        path = os.path.join(args.output_dir, f"{name}.ansi")
        with open(path, "w", encoding="utf-8") as f:
            f.write(content)
        size_kb = os.path.getsize(path) / 1024
        print(f"  {path} ({size_kb:.0f} KB)")

    print("Done.")


if __name__ == "__main__":
    main()
