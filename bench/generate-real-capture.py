#!/usr/bin/env python3
"""Generate a synthetic 'real slow' agent capture fixture.

This simulates the output pattern that makes PlaneAI feel slow:
- Large bursts of code output (10-50KB at once)
- Rapid streaming token-by-token for thinking text
- Interleaved ANSI formatting from the agent's CLI
- Long file diffs with heavy color
- Progress spinners that update rapidly

The fixture is ~2MB, sized to stress the terminal pipeline.
"""

import os
import random
import sys

SEED = 12345
TARGET_BYTES = 2_500_000  # slightly larger than synthetic fixtures

# ANSI
RST = "\033[0m"
BOLD = "\033[1m"
DIM = "\033[2m"
GREEN = "\033[32m"
RED = "\033[31m"
YELLOW = "\033[33m"
BLUE = "\033[34m"
CYAN = "\033[36m"
MAGENTA = "\033[35m"
BG_BLACK = "\033[40m"
WHITE = "\033[37m"


def agent_header(rng):
    """Kiro/Claude-style status line."""
    spinners = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
    lines = []
    # Rapid spinner updates (each overwrites the line)
    for i in range(rng.randint(20, 60)):
        s = spinners[i % len(spinners)]
        elapsed = rng.randint(1, 300)
        msg = rng.choice(["Thinking...", "Reading files...", "Planning...", "Analyzing..."])
        lines.append(f"\r{CYAN}{s}{RST} {msg} ({elapsed}s)")
    lines.append(f"\r{GREEN}✓{RST} Done thinking\r\n\r\n")
    return "".join(lines)


def code_block(rng, lang, line_count):
    """Generate a large code block with syntax-highlighting-like ANSI."""
    lines = [f"{DIM}```{lang}{RST}\r\n"]
    keywords = {
        "rust": ["fn", "let", "mut", "pub", "struct", "impl", "use", "mod", "async", "await", "match", "if", "else", "return"],
        "typescript": ["const", "let", "function", "export", "import", "async", "await", "if", "else", "return", "interface", "type"],
        "python": ["def", "class", "import", "from", "if", "else", "return", "async", "await", "with", "for", "in"],
    }
    kws = keywords.get(lang, keywords["typescript"])
    idents = ["handle_request", "process_data", "render_frame", "parse_input", "emit_event",
              "write_output", "flush_buffer", "update_state", "check_condition", "transform"]
    types = ["Result<()>", "String", "Vec<u8>", "Option<T>", "bool", "usize", "i32"]

    for _ in range(line_count):
        indent = "    " * rng.randint(0, 4)
        kw = rng.choice(kws)
        ident = rng.choice(idents)
        typ = rng.choice(types)
        comment = f" {DIM}// TODO: refactor this{RST}" if rng.random() < 0.1 else ""
        line = f"{indent}{BLUE}{kw}{RST} {ident}({YELLOW}{typ}{RST}){comment}\r\n"
        lines.append(line)

    lines.append(f"{DIM}```{RST}\r\n\r\n")
    return "".join(lines)


def diff_block(rng, line_count):
    """Generate a git diff-like output with heavy color."""
    path = rng.choice(["src/lib/session.ts", "src-tauri/src/pty.rs", "src/App.svelte",
                       "src/components/Terminal.svelte", "src/lib/benchmark/replay.ts"])
    lines = [f"{BOLD}diff --git a/{path} b/{path}{RST}\r\n"]
    lines.append(f"{CYAN}@@ -{rng.randint(1,200)},{rng.randint(5,30)} +{rng.randint(1,200)},{rng.randint(5,30)} @@{RST}\r\n")

    for _ in range(line_count):
        r = rng.random()
        content = " " * rng.randint(0, 8) + rng.choice([
            "let result = process(input);",
            "const data = await fetch(url);",
            "if (condition) { return; }",
            "fn handle(&mut self, event: Event) {",
            "    term.write(chunk, callback);",
            "pub async fn replay_file(",
            "    pendingBytes += data.byteLength;",
        ])
        if r < 0.3:
            lines.append(f"{RED}-{content}{RST}\r\n")
        elif r < 0.6:
            lines.append(f"{GREEN}+{content}{RST}\r\n")
        else:
            lines.append(f" {content}\r\n")
    lines.append("\r\n")
    return "".join(lines)


def thinking_stream(rng, word_count):
    """Token-by-token thinking output (small chunks, lots of ANSI resets)."""
    words = ("the implementation needs to handle edge cases where the terminal "
             "buffer overflows and we need backpressure from the renderer to the "
             "parser thread to prevent unbounded memory growth while maintaining "
             "smooth frame delivery at 60fps even under heavy ANSI escape sequence "
             "load from concurrent agent outputs streaming through the PTY layer "
             "into the xterm.js write queue which coalesces micro-writes into "
             "larger batches before triggering a render pass").split()
    out = []
    for i in range(word_count):
        w = rng.choice(words)
        # Occasional formatting
        if rng.random() < 0.05:
            out.append(f"{BOLD}{w}{RST} ")
        elif rng.random() < 0.03:
            out.append(f"`{CYAN}{w}{RST}` ")
        else:
            out.append(f"{w} ")
        # Simulate line wraps at ~80 chars
        if (i + 1) % 15 == 0:
            out.append("\r\n")
    out.append("\r\n\r\n")
    return "".join(out)


def error_block(rng):
    """Compiler/linter error output."""
    files = ["src/main.rs", "src/lib/api.ts", "src/components/Terminal.svelte"]
    lines = [f"{RED}{BOLD}error[E0308]{RST}: mismatched types\r\n"]
    lines.append(f"  {BLUE}-->{RST} {rng.choice(files)}:{rng.randint(10,500)}:{rng.randint(1,40)}\r\n")
    lines.append(f"   {DIM}|{RST}\r\n")
    for _ in range(rng.randint(3, 8)):
        ln = rng.randint(40, 500)
        lines.append(f"{DIM}{ln:3} |{RST}     let x = some_complex_expression();\r\n")
    lines.append(f"   {DIM}|{RST}              {RED}^^^^^^^^^^^^^^^^^^^^^^^^{RST}\r\n")
    lines.append(f"   {DIM}|{RST}              {RED}expected `usize`, found `&str`{RST}\r\n\r\n")
    return "".join(lines)


def build_output(rng, line_count):
    """Cargo/pnpm build output with rapid status updates."""
    crates = ["planeai-core", "planeai-daemon", "planeai-ipc", "planeai-tasks",
              "tokio", "serde", "tauri", "rusqlite", "iced", "alacritty_terminal"]
    lines = []
    for i in range(line_count):
        crate = rng.choice(crates)
        action = rng.choice(["Compiling", "Downloading", "Checking", "Building"])
        lines.append(f"   {GREEN}{action}{RST} {crate} v{rng.randint(0,2)}.{rng.randint(0,30)}.{rng.randint(0,10)}\r\n")
    return "".join(lines)


def generate_agent_real_slow():
    rng = random.Random(SEED)
    parts = []
    size = 0

    # Opening burst: agent starts, shows thinking spinner
    parts.append(agent_header(rng))
    size += len(parts[-1])

    while size < TARGET_BYTES:
        block_type = rng.choices(
            ["code", "diff", "thinking", "error", "build", "header"],
            weights=[35, 25, 20, 8, 7, 5],
        )[0]

        if block_type == "code":
            lang = rng.choice(["rust", "typescript", "python"])
            lines = rng.randint(20, 80)
            parts.append(code_block(rng, lang, lines))
        elif block_type == "diff":
            parts.append(diff_block(rng, rng.randint(15, 60)))
        elif block_type == "thinking":
            parts.append(thinking_stream(rng, rng.randint(50, 200)))
        elif block_type == "error":
            parts.append(error_block(rng))
        elif block_type == "build":
            parts.append(build_output(rng, rng.randint(10, 40)))
        elif block_type == "header":
            parts.append(agent_header(rng))

        size += len(parts[-1])

    return "".join(parts)


def main():
    output_dir = sys.argv[1] if len(sys.argv) > 1 else "bench/captures"
    os.makedirs(output_dir, exist_ok=True)
    path = os.path.join(output_dir, "agent-real-slow.ansi")

    content = generate_agent_real_slow()
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)

    size_kb = os.path.getsize(path) / 1024
    print(f"  {path} ({size_kb:.0f} KB)")


if __name__ == "__main__":
    main()
