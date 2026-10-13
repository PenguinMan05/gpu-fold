#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  if [[ "${1:-}" != "--approve-image-download" ]]; then
    echo "This command downloads the official Rust container base and builds the local sandbox image."
    echo "Run with --approve-image-download only after reviewing scripts/Containerfile."
    exit 2
  fi
  if [[ "$(podman info --format '{{.Host.Security.Rootless}}')" != true ]]; then
    echo "ERROR: run Podman rootless as your regular desktop user."
    exit 2
  fi
  podman build --pull=missing -t localhost/rust-repair-sandbox:1 -f scripts/Containerfile .
  echo "SUCCESS: sandbox image built; app test runs never download images."
)
