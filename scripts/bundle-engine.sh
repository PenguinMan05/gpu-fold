#!/usr/bin/env bash
# Build/release task only. The installed app never downloads an executable.
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  engine_tag=b11627
  engine_sha256=e5eaae62de38776a837d1f415a461dd334342c1a247073c51a065e04ff773c1f
  engine_url="https://github.com/ggml-org/llama.cpp/releases/download/${engine_tag}/llama-${engine_tag}-bin-ubuntu-vulkan-x64.tar.gz"
  engine_tmp="$(mktemp -d)"
  trap 'rm -rf "$engine_tmp"' EXIT
  curl --fail --location --retry 3 --connect-timeout 20 --max-time 240 "$engine_url" -o "$engine_tmp/engine.tar.gz"
  echo "$engine_sha256  $engine_tmp/engine.tar.gz" | sha256sum -c -
  mkdir -p "$engine_tmp/unpacked" dist/engine
  tar -xzf "$engine_tmp/engine.tar.gz" -C "$engine_tmp/unpacked" --no-same-owner
  server="$(find "$engine_tmp/unpacked" -type f -name llama-server -print -quit)"
  test -n "$server"
  install -m 755 "$server" dist/engine/llama-server
  while IFS= read -r -d '' library; do
    install -m 755 "$library" "dist/engine/$(basename "$library")"
  done < <(find -L "$engine_tmp/unpacked" -type f -name '*.so*' -print0)
  install -m 644 third-party/llama.cpp-LICENSE.txt dist/engine/LICENSE.txt
  # Preserve any additional license notices shipped by upstream.
  mkdir -p dist/engine/notices
  while IFS= read -r -d '' notice; do
    install -m 644 "$notice" "dist/engine/notices/$(basename "$notice")"
  done < <(find "$engine_tmp/unpacked" -type f \( -iname '*license*' -o -iname '*notice*' \) -print0)
  cat > dist/engine/UPSTREAM.json <<META
{"project":"llama.cpp","tag":"$engine_tag","commit":"8c2134a8755d8a9792af513dab20282affe903e4","archive":"$engine_url","sha256":"$engine_sha256","backend":"Vulkan and CPU","license":"MIT"}
META
  export LD_LIBRARY_PATH="$PWD/dist/engine"
  export GGML_BACKEND_PATH="$PWD/dist/engine"
  dist/engine/llama-server --version
  dist/engine/llama-server --list-devices > dist/engine-devices.txt 2>&1
  ldd dist/engine/llama-server dist/engine/*.so* > dist/engine-libraries.txt
  if rg 'not found' dist/engine-libraries.txt; then
    echo 'ERROR: a bundled engine dependency is missing.'
    exit 1
  fi
  test -n "$(find dist/engine -name '*vulkan*.so*' -print -quit)"
  echo 'SUCCESS: verified GPU/CPU inference engine prepared for packaging.'
)
