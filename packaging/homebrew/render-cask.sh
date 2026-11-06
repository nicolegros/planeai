#!/usr/bin/env bash
# Renders the planeai cask for a release and prints it to stdout.
# Usage: render-cask.sh <version> <aarch64.dmg>
set -euo pipefail

die() {
  echo "render-cask: $*" >&2
  exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 <version> <aarch64.dmg>"
version="$1"
dmg="$2"

[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
  die "version must be MAJOR.MINOR.PATCH without a leading v, got '$version'"
[[ -s "$dmg" ]] || die "aarch64 dmg missing or empty: $dmg"

sha="$(shasum -a 256 "$dmg" | cut -d ' ' -f 1)"

template="$(dirname "$0")/planeai.rb.tmpl"
rendered="$(sed -e "s/@VERSION@/$version/" \
  -e "s/@SHA256@/$sha/" \
  "$template")"
if grep -q '@[A-Z0-9_]*@' <<<"$rendered"; then
  die "unrendered placeholder left in $template"
fi
printf '%s\n' "$rendered"
