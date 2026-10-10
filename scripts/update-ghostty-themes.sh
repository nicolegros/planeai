#!/usr/bin/env bash
# Refreshes src-tauri/resources/ghostty-themes from mbadolato/iTerm2-Color-Schemes (MIT).
# Usage: scripts/update-ghostty-themes.sh [commit-sha]
set -euo pipefail

SHA="${1:-c02052159ff9a1c438ff419b7451cd33f9ab4934}"
REPO="mbadolato/iTerm2-Color-Schemes"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/resources/ghostty-themes"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://codeload.github.com/$REPO/tar.gz/$SHA" -o "$TMP/repo.tar.gz"
mkdir "$TMP/src"
tar -xzf "$TMP/repo.tar.gz" -C "$TMP/src" --strip-components=1 "iTerm2-Color-Schemes-$SHA/ghostty" "iTerm2-Color-Schemes-$SHA/LICENSE"

rm -rf "$DEST"
mkdir -p "$DEST"
cp "$TMP/src/ghostty/"* "$DEST/"
cp "$TMP/src/LICENSE" "$DEST/LICENSE"

echo "Updated $(find "$DEST" -type f ! -name LICENSE | wc -l | tr -d ' ') Ghostty themes from $REPO@$SHA"
