#!/usr/bin/env bash
# src-tauri/scripts/ensure-sidecars.sh
#
# Ensures the fixed external-bin set is ready in src-tauri/binaries/. The
# placeholders must exist before Cargo compiles the Tauri host build script;
# ordinary invocations then replace them with compiled release binaries.
set -euo pipefail

PLACEHOLDERS_ONLY="${1:-}"
if [[ -n "$PLACEHOLDERS_ONLY" && "$PLACEHOLDERS_ONLY" != "--placeholders-only" ]]; then
  echo "usage: $0 [--placeholders-only]" >&2
  exit 2
fi

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

if [[ "$PLACEHOLDERS_ONLY" == "--placeholders-only" ]]; then
  exit 0
fi

cargo build --release --target "$TARGET" \
  -p planeai-cli-bin \
  -p planeai-daemon-bin \
  -p planeai-plugin-jira

for bin in planeai-cli planeai-daemon planeai-plugin-jira; do
  cp "target/${TARGET}/release/${bin}${EXT}" "binaries/${bin}-${TARGET}${EXT}"
done

if [[ "$EXT" != ".exe" ]]; then
  chmod +x binaries/planeai-cli-"${TARGET}" \
    binaries/planeai-daemon-"${TARGET}" \
    binaries/planeai-plugin-jira-"${TARGET}"
fi
