#!/usr/bin/env python3
"""Generate ANSI fixture files for the Iced terminal spike benchmark."""

import os
import random
import sys

FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "fixtures")


def ensure_dir():
    os.makedirs(FIXTURES_DIR, exist_ok=True)


def write_fixture(name: str, data: bytes):
    path = os.path.join(FIXTURES_DIR, name)
    with open(path, "wb") as f:
        f.write(data)
    print(f"  {path} ({len(data)} bytes)")


def generate_ansi_flood(size: int = 2 * 1024 * 1024) -> bytes:
    """High-throughput plain text with occasional color resets."""
    chunks = []
    written = 0
    line = 0
    while written < size:
        color = f"\x1b[{random.choice([31,32,33,34,35,36,37])}m"
        text = f"line {line:06d}: " + "A" * random.randint(40, 100) + "\x1b[0m\r\n"
        chunk = (color + text).encode()
        chunks.append(chunk)
        written += len(chunk)
        line += 1
    return b"".join(chunks)[:size]


def generate_long_lines(size: int = 2 * 1024 * 1024) -> bytes:
    """Lines that exceed terminal width, forcing wrapping."""
    chunks = []
    written = 0
    while written < size:
        length = random.randint(200, 500)
        line = "X" * length + "\r\n"
        chunks.append(line.encode())
        written += length + 2
    return b"".join(chunks)[:size]


def generate_colors_heavy(size: int = 2 * 1024 * 1024) -> bytes:
    """Dense 256-color and truecolor sequences."""
    chunks = []
    written = 0
    while written < size:
        mode = random.randint(0, 2)
        if mode == 0:
            # 256-color foreground
            c = random.randint(0, 255)
            seq = f"\x1b[38;5;{c}m#{c:02x}\x1b[0m"
        elif mode == 1:
            # truecolor foreground
            r, g, b = random.randint(0, 255), random.randint(0, 255), random.randint(0, 255)
            seq = f"\x1b[38;2;{r};{g};{b}m\u2588\x1b[0m"
        else:
            # 256-color background
            c = random.randint(0, 255)
            seq = f"\x1b[48;5;{c}m \x1b[0m"
        encoded = seq.encode()
        chunks.append(encoded)
        written += len(encoded)
        if random.random() < 0.05:
            chunks.append(b"\r\n")
            written += 2
    return b"".join(chunks)[:size]


def generate_progress_bars(size: int = 2 * 1024 * 1024) -> bytes:
    """Simulated progress bars with carriage returns."""
    chunks = []
    written = 0
    task = 0
    while written < size:
        task += 1
        # Simulate a progress bar going 0..100%
        for pct in range(0, 101, random.randint(1, 5)):
            bar_width = 50
            filled = int(bar_width * pct / 100)
            bar = "\u2588" * filled + "\u2591" * (bar_width - filled)
            line = f"\r\x1b[32m[{bar}] {pct:3d}% task-{task:04d}\x1b[0m"
            encoded = line.encode()
            chunks.append(encoded)
            written += len(encoded)
            if written >= size:
                break
        chunks.append(b"\r\n")
        written += 2
    return b"".join(chunks)[:size]


def generate_mixed_agent_like(size: int = 2 * 1024 * 1024) -> bytes:
    """Simulates a coding agent session: prompts, code blocks, diffs, spinners, status lines."""
    chunks = []
    written = 0
    step = 0
    languages = ["rust", "python", "typescript", "go"]
    while written < size:
        step += 1
        mode = random.randint(0, 5)
        if mode == 0:
            # Agent thinking spinner
            for frame in ["|", "/", "-", "\\"]:
                line = f"\r\x1b[33m⟳ Thinking... {frame}\x1b[0m"
                chunks.append(line.encode())
                written += len(line)
        elif mode == 1:
            # Code block with syntax-highlighted output
            lang = random.choice(languages)
            header = f"\x1b[1;36m```{lang}\x1b[0m\r\n"
            chunks.append(header.encode())
            written += len(header)
            for _ in range(random.randint(5, 20)):
                indent = "    " * random.randint(0, 3)
                kw = random.choice(["\x1b[1;35mfn\x1b[0m", "\x1b[1;35mdef\x1b[0m",
                                    "\x1b[1;35mlet\x1b[0m", "\x1b[1;35mimport\x1b[0m"])
                code = f"{indent}{kw} {random.choice(['foo','bar','process','handle'])}_{step}" \
                       f"(\x1b[33m{random.randint(0,100)}\x1b[0m)\r\n"
                chunks.append(code.encode())
                written += len(code)
        elif mode == 2:
            # Diff output
            for _ in range(random.randint(3, 10)):
                if random.random() < 0.5:
                    line = f"\x1b[32m+ added line {step} {random.choice(['impl','test','fix'])}\x1b[0m\r\n"
                else:
                    line = f"\x1b[31m- removed line {step}\x1b[0m\r\n"
                chunks.append(line.encode())
                written += len(line)
        elif mode == 3:
            # Status/progress line (carriage return overwrite)
            for i in range(random.randint(5, 15)):
                line = f"\r\x1b[2K\x1b[34m[{i+1}/15]\x1b[0m Processing file_{step}_{i}.rs..."
                chunks.append(line.encode())
                written += len(line)
            chunks.append(b"\r\n")
            written += 2
        elif mode == 4:
            # Prompt/response with bold
            prompt = f"\x1b[1;32m❯\x1b[0m \x1b[1m$ cargo test --release\x1b[0m\r\n"
            chunks.append(prompt.encode())
            written += len(prompt)
            for _ in range(random.randint(2, 8)):
                result = f"  \x1b[32mtest\x1b[0m step_{step}::{random.choice(['ok','ok','ok','FAILED'])} " \
                         f"... \x1b[{'32' if random.random() > 0.1 else '31'}m" \
                         f"{'ok' if random.random() > 0.1 else 'FAILED'}\x1b[0m\r\n"
                chunks.append(result.encode())
                written += len(result)
        else:
            # Plain explanatory text (like agent explanation)
            text = f"I'll now modify the `handle_{step}` function to fix the " \
                   f"{'memory leak' if step % 2 == 0 else 'race condition'}. " \
                   f"This requires updating {random.randint(2,5)} files.\r\n"
            chunks.append(text.encode())
            written += len(text)

        if written < size:
            chunks.append(b"\r\n")
            written += 2

    return b"".join(chunks)[:size]


def main():
    size = int(sys.argv[1]) if len(sys.argv) > 1 else 2 * 1024 * 1024
    print(f"Generating fixtures (~{size // 1024}KB each) in {FIXTURES_DIR}/")
    ensure_dir()
    write_fixture("ansi-flood.ansi", generate_ansi_flood(size))
    write_fixture("long-lines.ansi", generate_long_lines(size))
    write_fixture("colors-heavy.ansi", generate_colors_heavy(size))
    write_fixture("progress-bars.ansi", generate_progress_bars(size))
    write_fixture("mixed-agent-like.ansi", generate_mixed_agent_like(size))
    print("Done.")


if __name__ == "__main__":
    main()
