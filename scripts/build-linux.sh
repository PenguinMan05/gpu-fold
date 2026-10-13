#!/usr/bin/env bash
# Runs in a subshell; errors do not close the calling terminal.
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: Rust/Cargo is required. Install Rust 1.90+ with rustup, then run this script again."
    exit 2
  fi
  test -f Cargo.lock
  cargo fmt --all --check
  cargo test --locked -p repair-core
  cargo check --locked --workspace
  cargo build --locked --release --workspace
  mkdir -p dist
  cp target/release/rust-repair target/release/rust-repair-desktop dist/
  ./dist/rust-repair --version
  echo "SUCCESS: verified build produced in dist/"
  if [[ "${1:-}" == "--install" ]]; then
    app_bin="${HOME}/.local/bin"
    app_desktop="${HOME}/.local/share/applications"
    mkdir -p "$app_bin" "$app_desktop"
    install -m 755 dist/rust-repair dist/rust-repair-desktop "$app_bin/"
    # Quote the executable path for the desktop entry, not a shell command.
    desktop_exe="${app_bin}/rust-repair-desktop"
    desktop_exe="${desktop_exe//\\/\\\\}"
    desktop_exe="${desktop_exe//\"/\\\"}"
    desktop_exe="${desktop_exe//\$/\\\$}"
    desktop_exe="${desktop_exe//\`/\\\`}"
    desktop_exe="${desktop_exe//%/%%}"
    cat > "$app_desktop/rust-repair.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Rust Repair
Comment=Review, test, and propose Rust repository fixes using local AI
Exec="$desktop_exe"
Icon=applications-development
Terminal=false
Categories=Development;
DESKTOP
    echo "SUCCESS: installed Rust Repair to your application menu"
  fi
)
