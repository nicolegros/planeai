#!/usr/bin/env python3
"""Continuous colored terminal output for flood/responsiveness testing.

Emits deterministic colored lines with occasional carriage-return progress
updates until interrupted with Ctrl-C (KeyboardInterrupt).
"""
import sys

COLORS = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96]

def main():
    i = 0
    try:
        while True:
            color = COLORS[i % len(COLORS)]
            line = f"\033[{color}m[{i:08d}] PlaneAI flood test — the quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstuvwxyz\033[0m"
            print(line, flush=True)
            if i % 50 == 49:
                # Carriage-return progress update (overwrites current line)
                sys.stdout.write(f"\r\033[33m  progress: {i + 1} lines emitted\033[0m")
                sys.stdout.flush()
            i += 1
    except (KeyboardInterrupt, BrokenPipeError):
        sys.stdout.write("\n")
        sys.exit(0)

if __name__ == "__main__":
    main()
