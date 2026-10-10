#!/usr/bin/env bash
# Refreshes src-tauri/resources/ghostty-themes from mbadolato/iTerm2-Color-Schemes (MIT).
# Usage: scripts/update-ghostty-themes.sh [commit-sha]   (defaults to the latest upstream commit)
# The bundled commit is recorded in src-tauri/resources/ghostty-themes/UPSTREAM.
# .github/workflows/update-ghostty-themes.yml runs this weekly and opens a PR when upstream changed.
set -euo pipefail

REPO="mbadolato/iTerm2-Color-Schemes"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/resources/ghostty-themes"

SHA="${1:-$(curl -fsSL -H "Accept: application/vnd.github.sha" "https://api.github.com/repos/$REPO/commits/master")}"
if [[ ! "$SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Expected a full 40-character commit SHA, got: $SHA" >&2
  exit 1
fi

PREVIOUS="$(cat "$DEST/UPSTREAM" 2>/dev/null || echo none)"
if [[ "$PREVIOUS" == "$SHA" ]]; then
  echo "Ghostty themes are already at $REPO@$SHA"
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://codeload.github.com/$REPO/tar.gz/$SHA" -o "$TMP/repo.tar.gz"
mkdir "$TMP/src"
tar -xzf "$TMP/repo.tar.gz" -C "$TMP/src" --strip-components=1 "iTerm2-Color-Schemes-$SHA/ghostty" "iTerm2-Color-Schemes-$SHA/LICENSE" "iTerm2-Color-Schemes-$SHA/CREDITS.md"

rm -rf "$DEST"
mkdir -p "$DEST"
cp "$TMP/src/ghostty/"* "$DEST/"
cp "$TMP/src/LICENSE" "$DEST/LICENSE"
cp "$TMP/src/CREDITS.md" "$DEST/CREDITS.md"
echo "$SHA" > "$DEST/UPSTREAM"

echo "Updated $(find "$DEST" -type f ! -name LICENSE ! -name CREDITS.md ! -name UPSTREAM | wc -l | tr -d ' ') Ghostty themes from $REPO@$SHA (was $PREVIOUS)"
