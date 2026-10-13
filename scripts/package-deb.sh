#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  test -x dist/rust-repair-desktop
  test -x dist/rust-repair
  dist/rust-repair --version
  package_dir="$(mktemp -d)"
  trap 'rm -rf "$package_dir"' EXIT
  mkdir -p "$package_dir/DEBIAN" "$package_dir/usr/bin" "$package_dir/usr/share/applications" "$package_dir/usr/share/doc/rust-repair" "$package_dir/usr/share/rust-repair/scripts"
  install -m 755 dist/rust-repair dist/rust-repair-desktop "$package_dir/usr/bin/"
  install -m 644 README.md LICENSE "$package_dir/usr/share/doc/rust-repair/"
  glibc_min="$(readelf --version-info dist/rust-repair dist/rust-repair-desktop | sed -nE 's/.*Name: GLIBC_([0-9.]+).*/\1/p' | sort -V | tail -n 1)"
  test -n "$glibc_min"
  install -m 644 scripts/Containerfile scripts/sandbox-runner.rs "$package_dir/usr/share/rust-repair/scripts/"
  install -m 755 scripts/build-sandbox.sh "$package_dir/usr/share/rust-repair/scripts/"
  cat > "$package_dir/DEBIAN/control" <<CONTROL
Package: rust-repair
Version: 0.1.0
Architecture: amd64
Maintainer: Rust Repair contributors
Depends: libc6 (>= ${glibc_min}), libgcc-s1, libx11-6, libxkbcommon0, libxkbcommon-x11-0, libgl1, libwayland-client0, libdbus-1-3, git, podman
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
  dpkg-deb --info dist/rust-repair_0.1.0_amd64.deb
  echo "SUCCESS: local .deb package created; test it on your target distribution before sharing."
)
