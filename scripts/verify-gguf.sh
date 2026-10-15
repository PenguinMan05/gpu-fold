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
  # This toy model declares a 128-token training window, too short even for the
  # required JSON field names. Expand only the CI copy's declared context to test
  # the inference protocol/grammar. This is not a coding-quality evaluation.
  python3 - "$test_dir/stories260K.gguf" <<'PYMODEL'
from pathlib import Path
import struct,sys
path=Path(sys.argv[1]); data=bytearray(path.read_bytes()); key=b'llama.context_length'
at=data.index(key)
assert struct.unpack_from('<Q',data,at-8)[0]==len(key)
assert struct.unpack_from('<I',data,at+len(key))[0]==4
assert struct.unpack_from('<I',data,at+len(key)+4)[0]==128
struct.pack_into('<I',data,at+len(key)+4,2048)
path.write_bytes(data)
print('CI fixture only: declared context 128 -> 2048 tokens; weights unchanged.')
PYMODEL
  export GPUFOLD_TEST_GGUF="$test_dir/stories260K.gguf"
  cargo test --locked -p repair-core --test bundled_runtime -- --ignored --nocapture
  if [[ -z "${GPUFOLD_TEST_INSTALLED:-}" ]]; then
    cargo test --locked -p repair-desktop --bin rust-repair-desktop -- --ignored --nocapture
  fi
)
