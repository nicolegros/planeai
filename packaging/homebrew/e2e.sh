#!/usr/bin/env bash
# End to end check of the planeai cask: install, CLI on PATH, legacy symlink cleanup and zap.
# DESTRUCTIVE: installs over /Applications/planeai.app, kills planeai-daemon and trashes the
# current user's planeai data. Run it only on a throwaway machine such as a CI runner.
# Usage: e2e.sh <tap/cask, e.g. planeai-ci/local/planeai>
set -euo pipefail

fail() {
  echo "e2e FAIL: $*" >&2
  exit 1
}
ok() {
  echo "e2e ok: $*"
}

[[ $# -eq 1 ]] || fail "usage: $0 <tap/cask>"
cask="$1"

app=/Applications/planeai.app
macos_dir="$app/Contents/MacOS"
cli_link="$(brew --prefix)/bin/planeai-cli"
legacy_dir=/usr/local/bin
legacy_cli="$legacy_dir/planeai-cli"
decoy="$legacy_dir/planeai-daemon"
decoy_target=/usr/bin/true
socket="/tmp/planeai-$(id -u)/daemon.sock"
zap_paths=(
  "$HOME/.config/planeai"
  "$HOME/.planeai"
  "$HOME/Library/Application Support/ca.nicolegros.planeai"
  "$HOME/Library/Caches/ca.nicolegros.planeai"
  "$HOME/Library/HTTPStorages/ca.nicolegros.planeai"
  "$HOME/Library/Preferences/ca.nicolegros.planeai.plist"
  "$HOME/Library/Saved Application State/ca.nicolegros.planeai.savedState"
  "$HOME/Library/WebKit/ca.nicolegros.planeai"
  "/tmp/planeai-$(id -u)"
)

in_legacy_dir() {
  if [[ -w "$legacy_dir" ]]; then "$@"; else sudo "$@"; fi
}

daemon_running() {
  pgrep -x planeai-daemon >/dev/null
}

brew install --cask "$cask"
[[ -d "$app" ]] || fail "$app missing after install"
ok "$app installed"
[[ "$(readlink "$cli_link")" == "$macos_dir/planeai-cli" ]] ||
  fail "$cli_link does not link to $macos_dir/planeai-cli (got '$(readlink "$cli_link")')"
"$cli_link" --help >/dev/null || fail "$cli_link --help exited non-zero"
ok "$cli_link links into the bundle and runs"

[[ -d "$legacy_dir" ]] || sudo mkdir -p "$legacy_dir"
in_legacy_dir ln -sfn "$macos_dir/planeai-cli" "$legacy_cli"
in_legacy_dir ln -sfn "$decoy_target" "$decoy"
for path in "${zap_paths[@]}"; do
  if [[ "$path" == *.plist ]]; then
    mkdir -p "$(dirname "$path")"
    touch "$path"
  else
    mkdir -p "$path"
  fi
done

nohup "$macos_dir/planeai-daemon" >/dev/null 2>&1 &
for _ in $(seq 50); do
  [[ -S "$socket" ]] && break
  sleep 0.2
done
[[ -S "$socket" ]] || fail "planeai-daemon did not create $socket"
daemon_running || fail "planeai-daemon not running after start"
ok "planeai-daemon running"

brew uninstall --zap --cask "$cask"

[[ ! -e "$app" ]] || fail "$app still present after zap"
[[ ! -L "$cli_link" ]] || fail "$cli_link still present after uninstall"
[[ ! -L "$legacy_cli" ]] || fail "legacy $legacy_cli into the bundle not removed"
[[ "$(readlink "$decoy")" == "$decoy_target" ]] || fail "decoy $decoy pointing outside the bundle was touched"
for path in "${zap_paths[@]}"; do
  [[ ! -e "$path" ]] || fail "zap left $path behind"
done
for _ in $(seq 25); do
  daemon_running || break
  sleep 0.2
done
! daemon_running || fail "planeai-daemon still running after zap"
ok "uninstall --zap removed app, CLI link, legacy symlink, data and daemon; decoy kept"

in_legacy_dir rm -f "$decoy"
