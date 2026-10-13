#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  test -x dist/rust-repair-desktop
  test -x dist/rust-repair
  dist/rust-repair --version
  package_dir="$(mktemp -d)"
  trap 'rm -rf "$package_dir"' EXIT
  mkdir -p "$package_dir/DEBIAN" "$package_dir/usr/bin" "$package_dir/usr/share/applications" "$package_dir/usr/share/doc/rust-repair"
  install -m 755 dist/rust-repair dist/rust-repair-desktop "$package_dir/usr/bin/"
  install -m 644 README.md LICENSE "$package_dir/usr/share/doc/rust-repair/"
  cat > "$package_dir/DEBIAN/control" <<'CONTROL'
Package: rust-repair
Version: 0.1.0
Architecture: amd64
Maintainer: Rust Repair contributors
Depends: libc6 (>= 2.35), libgcc-s1, libx11-6, libxkbcommon0, libgl1, libwayland-client0, libdbus-1-3, git, podman
Description: Local AI-assisted Rust repository repair desktop app
 Requires a separately installed local inference server and a built sandbox image.
CONTROL
  cat > "$package_dir/usr/share/applications/rust-repair.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Rust Repair
Exec=/usr/bin/rust-repair-desktop
Icon=applications-development
Terminal=false
Categories=Development;
DESKTOP
  dpkg-deb --build --root-owner-group "$package_dir" dist/rust-repair_0.1.0_amd64.deb
  echo "SUCCESS: local .deb package created; test it on your target distribution before sharing."
)
