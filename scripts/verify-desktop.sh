#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  mkdir -p dist
  desktop_test_data="$(mktemp -d)"
  export XDG_DATA_HOME="$desktop_test_data"
  export RUST_BACKTRACE=1
  ./dist/rust-repair-desktop >dist/desktop-startup.log 2>&1 &
  app_pid=$!
  trap 'kill "$app_pid" 2>/dev/null || true; rm -rf "$desktop_test_data"' EXIT
  app_window=""
  for _ in $(seq 1 45); do
    kill -0 "$app_pid"
    app_window="$(xdotool search --onlyvisible --name '^GPUFold$' 2>/dev/null | head -n 1 || true)"
    if [[ -n "$app_window" ]]; then break; fi
    sleep 1
  done
  if [[ -z "$app_window" ]]; then
    ps -o pid,stat,wchan,command -p "$app_pid" > dist/desktop-process.txt
    xwininfo -root -tree > dist/desktop-windows.txt
    import -window root dist/desktop-preview.png
    cat dist/desktop-startup.log dist/desktop-process.txt dist/desktop-windows.txt
    echo "ERROR: GPUFold did not show its window within 45 seconds."
    exit 1
  fi
  sleep 1
  import -window root dist/desktop-preview.png
  xdotool windowfocus --sync "$app_window"
  xdotool key ctrl+1
  sleep 1
  import -window root dist/dashboard-preview.png
  xdotool key ctrl+3
  sleep 1
  import -window root dist/issues-preview.png
  xdotool key ctrl+2
  sleep 1
  import -window root dist/setup-preview.png
  kill -0 "$app_pid"
  test -s dist/desktop-preview.png
  cmp -s dist/dashboard-preview.png dist/issues-preview.png && { echo "ERROR: navigation did not change the visible page"; exit 1; }
  cmp -s dist/issues-preview.png dist/setup-preview.png && { echo "ERROR: setup shortcut did not change the visible page"; exit 1; }
  echo "SUCCESS: native desktop remained running and a screenshot was captured."
)
