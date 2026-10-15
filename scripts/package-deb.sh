#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  test -x dist/rust-repair-desktop
  test -x dist/rust-repair
  dist/rust-repair --version
  test -x dist/engine/llama-server
  package_version="$(dist/rust-repair --version | awk '{print $NF}')"
  [[ "$package_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
  package_dir="$(mktemp -d)"
  trap 'rm -rf "$package_dir"' EXIT
  mkdir -p "$package_dir/DEBIAN" "$package_dir/usr/bin" "$package_dir/usr/share/applications" "$package_dir/usr/share/doc/rust-repair" "$package_dir/usr/share/rust-repair/scripts"
  install -m 755 dist/rust-repair dist/rust-repair-desktop "$package_dir/usr/bin/"
  ln -s rust-repair-desktop "$package_dir/usr/bin/gpufold"
  ln -s rust-repair "$package_dir/usr/bin/gpufold-cli"
  mkdir -p "$package_dir/usr/lib/gpufold/engine"
  cp -a dist/engine/. "$package_dir/usr/lib/gpufold/engine/"
  install -m 644 README.md LICENSE "$package_dir/usr/share/doc/rust-repair/"
  glibc_min="$(readelf --version-info dist/rust-repair dist/rust-repair-desktop dist/engine/llama-server dist/engine/*.so* | sed -nE 's/.*Name: GLIBC_([0-9.]+).*/\1/p' | sort -V | tail -n 1)"
  test -n "$glibc_min"
  install -m 644 scripts/Containerfile scripts/sandbox-runner.rs "$package_dir/usr/share/rust-repair/scripts/"
  install -m 755 scripts/build-sandbox.sh "$package_dir/usr/share/rust-repair/scripts/"
  cat > "$package_dir/DEBIAN/control" <<CONTROL
Package: gpufold
Version: ${package_version}
Conflicts: rust-repair
Replaces: rust-repair
Provides: rust-repair
Architecture: amd64
Maintainer: GPUFold contributors
Depends: libc6 (>= ${glibc_min}), libgcc-s1, libx11-6, libxkbcommon0, libxkbcommon-x11-0, libgl1, libwayland-client0, libdbus-1-3, libvulkan1, libgomp1, libstdc++6, libcurl4t64, libssl3t64, git, podman
Description: Local-model GitHub issue queue and Rust repair desktop app
 Includes a local GGUF inference engine with Vulkan GPU and CPU support.
CONTROL
  cat > "$package_dir/usr/share/applications/rust-repair.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=GPUFold
Exec=/usr/bin/gpufold
Comment=Set up a local AI model and work through GitHub issues
Icon=applications-development
Terminal=false
Categories=Development;
DESKTOP
  package_file="dist/GPUFold_${package_version}_amd64.deb"
  dpkg-deb --build --root-owner-group "$package_dir" "$package_file"
  package_hash="$(sha1sum "$package_file" | awk '{print $1}')"
  unique_package="dist/GPUFold_${package_version}_amd64-${package_hash}.deb"
  mv "$package_file" "$unique_package"
  (cd dist; sha256sum "$(basename "$unique_package")" > "$(basename "$unique_package").sha256")
  dpkg-deb --info "$unique_package"
  echo "SUCCESS: $unique_package and its SHA-256 checksum were created."
)
