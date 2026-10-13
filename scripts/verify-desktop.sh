#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  mkdir -p dist
  ./dist/rust-repair-desktop >dist/desktop-startup.log 2>&1 &
  app_pid=$!
  trap 'kill "$app_pid" 2>/dev/null || true' EXIT
  for _ in 1 2 3 4 5; do
    sleep 1
    kill -0 "$app_pid"
  done
  import -window root dist/desktop-preview.png
  test -s dist/desktop-preview.png
  echo "SUCCESS: native desktop remained running and a screenshot was captured."
)
