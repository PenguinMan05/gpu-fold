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
  app_window="$(xdotool search --onlyvisible --pid "$app_pid" | head -n 1)"
  test -n "$app_window"
  xdotool key --window "$app_window" ctrl+1
  sleep 1
  import -window root dist/dashboard-preview.png
  xdotool key --window "$app_window" ctrl+3
  sleep 1
  import -window root dist/issues-preview.png
  xdotool key --window "$app_window" ctrl+2
  sleep 1
  import -window root dist/setup-preview.png
  kill -0 "$app_pid"
  test -s dist/desktop-preview.png
  echo "SUCCESS: native desktop remained running and a screenshot was captured."
)
