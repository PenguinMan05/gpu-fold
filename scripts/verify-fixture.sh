#!/usr/bin/env bash
(
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  cli="${1:-./dist/rust-repair}"
  fixture_run="$("$cli" fixture)"
  "$cli" import-proposal "$fixture_run" fixtures/fix-add.json
  "$cli" approve-plan "$fixture_run"
  "$cli" baseline "$fixture_run" --approve-downloads-and-code-execution
  "$cli" apply "$fixture_run" --reviewed-patch
  "$cli" test "$fixture_run" --approve-downloads-and-code-execution
  mkdir -p dist
  "$cli" export "$fixture_run" dist/fixture-result.json
  python3 - <<'PY'
import json
with open('dist/fixture-result.json') as f:
    run = json.load(f)
assert run['evidence']['level'] == 3, run['evidence']['reason']
assert len(run['evidence']['baseline']) == 2
assert len(run['evidence']['after']) == 2
print('SUCCESS: dependency-free addition fixture reached level 3')
PY
)
