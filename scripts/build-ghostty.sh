#!/bin/bash
# Builds the GhosttyKit.xcframework from the vendored ghostty submodule.
# Requires: zig (install via `brew install zig`)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
GHOSTTY_DIR="$ROOT_DIR/vendor/ghostty"
OUTPUT_DIR="$ROOT_DIR/vendor/ghostty/macos"

# Prefer zig@0.15 (required by ghostty) over system zig
if [ -x "/opt/homebrew/opt/zig@0.15/bin/zig" ]; then
    export PATH="/opt/homebrew/opt/zig@0.15/bin:$PATH"
elif ! command -v zig &>/dev/null; then
    echo "error: zig 0.15.x is not installed. Run: brew install zig@0.15"
    exit 1
fi

echo "  zig version: $(zig version)"

echo "Building GhosttyKit.xcframework..."
echo "  ghostty dir: $GHOSTTY_DIR"
echo "  output dir:  $OUTPUT_DIR"

cd "$GHOSTTY_DIR"

# Build the xcframework for macOS (native arch only for dev speed)
zig build \
    -Demit-xcframework=true \
    -Dxcframework-target=native \
    -Doptimize=ReleaseFast

if [ -d "$OUTPUT_DIR/GhosttyKit.xcframework" ]; then
    echo "✓ GhosttyKit.xcframework built successfully"
    echo "  Location: $OUTPUT_DIR/GhosttyKit.xcframework"
else
    echo "error: GhosttyKit.xcframework not found after build"
    exit 1
fi
