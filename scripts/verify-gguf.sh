#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  test_dir="$(mktemp -d)"
  trap 'rm -rf "$test_dir"' EXIT
  curl --fail --location --retry 3 --connect-timeout 20 --max-time 120 \
    'https://huggingface.co/ggml-org/tiny-llamas/resolve/def3e2dd70df35ecbf6403ea347de4c5977220c1/stories260K.gguf' \
    -o "$test_dir/stories260K.gguf"
  echo "047bf46455a544931cff6fef14d7910154c56afbc23ab1c5e56a72e69912c04b  $test_dir/stories260K.gguf" | sha256sum -c -
  export GPUFOLD_TEST_GGUF="$test_dir/stories260K.gguf"
  cargo test --locked -p repair-core --test bundled_runtime -- --ignored --nocapture
  if [[ -z "${GPUFOLD_TEST_INSTALLED:-}" ]]; then
    cargo test --locked -p repair-desktop --bin rust-repair-desktop -- --ignored --nocapture
  fi
)
