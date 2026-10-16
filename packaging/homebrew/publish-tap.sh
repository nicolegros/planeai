#!/usr/bin/env bash
# Commits a rendered cask to the tap as Casks/planeai.rb and pushes it to main.
# Usage: HOMEBREW_TAP_DEPLOY_KEY=<private key> publish-tap.sh <rendered-cask>
set -euo pipefail

die() {
  echo "publish-tap: $*" >&2
  exit 1
}

[[ $# -eq 1 ]] || die "usage: $0 <rendered-cask>"
cask="$1"
[[ -s "$cask" ]] || die "rendered cask missing or empty: $cask"
[[ -n "${HOMEBREW_TAP_DEPLOY_KEY:-}" ]] || die "HOMEBREW_TAP_DEPLOY_KEY is not set"
version="$(sed -n 's/^  version "\(.*\)"$/\1/p' "$cask")"
[[ -n "$version" ]] || die "no version stanza in $cask"

remote="${HOMEBREW_TAP_REMOTE:-git@github.com:nicolegros/homebrew-tap.git}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

(
  umask 077
  printf '%s\n' "$HOMEBREW_TAP_DEPLOY_KEY" >"$work/deploy_key"
)
# https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/githubs-ssh-key-fingerprints
echo "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl" >"$work/known_hosts"
export GIT_SSH_COMMAND="ssh -i $work/deploy_key -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$work/known_hosts"

git clone --quiet --depth 1 --branch main "$remote" "$work/tap"
mkdir -p "$work/tap/Casks"
cp "$cask" "$work/tap/Casks/planeai.rb"

cd "$work/tap"
git add Casks/planeai.rb
if git diff --cached --quiet; then
  echo "publish-tap: Casks/planeai.rb already at planeai $version, nothing to push"
  exit 0
fi
git -c user.name="github-actions[bot]" \
  -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit --quiet -m "planeai $version"
git push --quiet origin HEAD:main
echo "publish-tap: pushed planeai $version"
