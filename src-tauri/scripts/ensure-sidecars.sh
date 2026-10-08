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

SIDECARS=(planeai-cli planeai-daemon planeai-plugin-jira)

mkdir -p binaries

for bin in "${SIDECARS[@]}"; do
  path="binaries/${bin}-${TARGET}${EXT}"
  if [[ ! -f "$path" ]]; then
    touch "$path"
    echo "Created placeholder: $path"
  fi
done

if [[ "$PLACEHOLDERS_ONLY" == "--placeholders-only" ]]; then
  exit 0
fi

# The Cargo package that produces each sidecar binary.
package_for_sidecar() {
  case "$1" in
    planeai-cli)         echo "planeai-cli-bin" ;;
    planeai-daemon)      echo "planeai-daemon-bin" ;;
    planeai-plugin-jira) echo "planeai-plugin-jira" ;;
    *) return 1 ;;
  esac
}

cargo build --release --target "$TARGET" \
  -p planeai-cli-bin \
  -p planeai-daemon-bin \
  -p planeai-plugin-jira

for bin in "${SIDECARS[@]}"; do
  src="target/${TARGET}/release/${bin}${EXT}"
  # Tauri's build script copies every `externalBin` out of binaries/ and into the
  # target directory, so the empty placeholder written above lands on top of a
  # previously compiled binary. When Cargo then considers that package fresh it
  # never rewrites it, and the copy below would publish an empty sidecar — which
  # fails only later, as a bundled binary that cannot execute. Discarding the
  # package's fingerprint forces the rebuild that restores it.
  if [[ ! -s "$src" ]]; then
    package="$(package_for_sidecar "$bin")"
    echo "Rebuilding $package: its artifact was replaced by a placeholder"
    cargo clean --release --target "$TARGET" -p "$package"
    cargo build --release --target "$TARGET" -p "$package"
  fi
  if [[ ! -s "$src" ]]; then
    echo "error: $src is empty after rebuilding; refusing to publish it." >&2
    exit 1
  fi
  cp "$src" "binaries/${bin}-${TARGET}${EXT}"
done

# A sidecar that is present but empty bundles a binary that cannot run, and the
# failure surfaces at launch rather than here.
for bin in "${SIDECARS[@]}"; do
  path="binaries/${bin}-${TARGET}${EXT}"
  if [[ ! -s "$path" ]]; then
    echo "error: sidecar $path is empty." >&2
    exit 1
  fi
done

if [[ "$EXT" != ".exe" ]]; then
  for bin in "${SIDECARS[@]}"; do
    chmod +x "binaries/${bin}-${TARGET}"
  done
fi
