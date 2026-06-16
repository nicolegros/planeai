#!/bin/bash
# Build the daemon binary and copy it to the location Tauri expects for externalBin.
set -euo pipefail

TRIPLE="${TAURI_ENV_TARGET_TRIPLE:-$(rustc -Vv | grep host | cut -d' ' -f2)}"
PROFILE="${1:-debug}"

echo "Building planeai-daemon (profile=$PROFILE, triple=$TRIPLE)"
if [ "$PROFILE" = "release" ]; then
    cargo build --release -p planeai-daemon
    SRC="target/release/planeai-daemon"
else
    cargo build -p planeai-daemon
    SRC="target/debug/planeai-daemon"
fi

DEST="binaries/planeai-daemon-${TRIPLE}"
mkdir -p binaries
cp "$SRC" "$DEST"
echo "Copied $SRC -> $DEST"
