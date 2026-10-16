#!/usr/bin/env bash
# Renders the planeai cask for a release and prints it to stdout.
# Usage: render-cask.sh <version> <aarch64.dmg> <x64.dmg>
set -euo pipefail

die() {
  echo "render-cask: $*" >&2
  exit 1
}

[[ $# -eq 3 ]] || die "usage: $0 <version> <aarch64.dmg> <x64.dmg>"
version="$1"
arm_dmg="$2"
intel_dmg="$3"

[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
  die "version must be MAJOR.MINOR.PATCH without a leading v, got '$version'"
[[ -s "$arm_dmg" ]] || die "aarch64 dmg missing or empty: $arm_dmg"
[[ -s "$intel_dmg" ]] || die "x64 dmg missing or empty: $intel_dmg"

sha256() {
  shasum -a 256 "$1" | cut -d ' ' -f 1
}

arm_sha="$(sha256 "$arm_dmg")"
intel_sha="$(sha256 "$intel_dmg")"

template="$(dirname "$0")/planeai.rb.tmpl"
rendered="$(sed -e "s/@VERSION@/$version/" \
  -e "s/@SHA256_ARM@/$arm_sha/" \
  -e "s/@SHA256_INTEL@/$intel_sha/" \
  "$template")"
if grep -q '@[A-Z0-9_]*@' <<<"$rendered"; then
  die "unrendered placeholder left in $template"
fi
printf '%s\n' "$rendered"
