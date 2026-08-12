#!/usr/bin/env bash
# src-tauri/scripts/ensure-sidecars.sh
#
# Ensures sidecar binary placeholders exist in src-tauri/binaries/ so that
# build.rs (tauri_build) passes validation. Release builds replace these with
# compiled target-suffixed binaries; debug builds resolve the Jira sidecar from
# its Cargo-built sibling and never fall back to PATH.
set -euo pipefail

TARGET="${TAURI_ENV_TARGET_TRIPLE:-$(rustc --print host-tuple)}"

EXT=""
if [[ "$TARGET" == *"windows"* ]]; then
  EXT=".exe"
fi

mkdir -p binaries

for bin in planeai-cli planeai-daemon planeai-plugin-jira; do
  path="binaries/${bin}-${TARGET}${EXT}"
  if [[ ! -f "$path" ]]; then
    touch "$path"
    echo "Created placeholder: $path"
  fi
done
