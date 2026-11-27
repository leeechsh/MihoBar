#!/usr/bin/env bash
# Runs the tests in a fresh data directory (MIHOBAR_HOME), with the panel
# shown. The tests start their own core with its controller on
# 127.0.0.1:9097 and its mixed port on 7897: when another MihoBar already
# holds them, the tests would drive that core instead, so this refuses.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
for port in 9097 7897; do
  # A connection, not lsof: a core the helper runs as root is invisible
  # to an unprivileged lsof.
  if nc -z -G 1 127.0.0.1 "$port" >/dev/null 2>&1; then
    echo "port $port is in use (a running MihoBar?): quit it before running the tests" >&2
    exit 1
  fi
done
# MihoBar ships no profile: the app's tests start from one of their own
# (tests/fixtures); the first-launch tests (Fresh.test.tsx) from none.
with_profile() {
  local home; home="$(mktemp -d)"
  mkdir -p "$home/config"
  cp tests/fixtures/Mihobar.yaml "$home/config/"
  MIHOBAR_HOME="$home" MIHOBAR_SHOW_PANEL=1 nsh test "$@"
}
if [ "$#" -gt 0 ]; then
  with_profile "$@"
  exit
fi
with_profile App.test.tsx
MIHOBAR_HOME="$(mktemp -d)" MIHOBAR_SHOW_PANEL=1 nsh test Fresh.test.tsx
