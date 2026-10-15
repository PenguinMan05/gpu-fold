#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  mkdir -p dist
  desktop_test_data="$(mktemp -d)"
  export XDG_DATA_HOME="$desktop_test_data"
  export RUST_BACKTRACE=1
  # Seed a clearly labeled sample conversation in the isolated UI-test history.
  preview_run="$(./dist/rust-repair fixture)"
  python3 - "$desktop_test_data/rust-repair/history.sqlite" "$preview_run" <<'PYUI'
import sqlite3,json,sys,time
connection=sqlite3.connect(sys.argv[1]); run_id=sys.argv[2]
run=json.loads(connection.execute('SELECT data FROM runs WHERE id=?',(run_id,)).fetchone()[0])
run['dialogue']=[
 {'role':'Task','text':'UI test sample: investigate an incorrect addition result.','at':int(time.time())},
 {'role':'Assistant','text':'I will inspect the implementation and its callers before proposing a minimal change.','at':int(time.time())},
 {'role':'Activity','text':'Opened src/lib.rs:1-40 from the pinned source.','at':int(time.time())},
 {'role':'Assistant','text':'The sample function subtracts its second operand. The proposed repair replaces subtraction with addition and adds a regression test.','at':int(time.time())},
 {'role':'Result','text':'Sample conversation for desktop verification. No model inference or repository changes occurred in this UI fixture.','at':int(time.time())}
]
connection.execute('UPDATE runs SET data=? WHERE id=?',(json.dumps(run),run_id));connection.commit()
PYUI
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
  xdotool key ctrl+4
  sleep 1
  import -window root dist/conversation-preview.png
  xdotool key ctrl+4
  sleep 1
  kill -0 "$app_pid"
  test -s dist/desktop-preview.png
  cmp -s dist/dashboard-preview.png dist/issues-preview.png && { echo "ERROR: navigation did not change the visible page"; exit 1; }
  cmp -s dist/issues-preview.png dist/setup-preview.png && { echo "ERROR: setup shortcut did not change the visible page"; exit 1; }
  cmp -s dist/setup-preview.png dist/conversation-preview.png && { echo "ERROR: conversation shortcut did not open the feed"; exit 1; }
  echo "SUCCESS: native desktop remained running and a screenshot was captured."
)
