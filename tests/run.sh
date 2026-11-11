#!/usr/bin/env bash
# Every test in the repository: the scripts' (tests/*/) and the two Rust crates'. No root, no
# network, nothing outside temporary directories. CARGO_TARGET_DIR is honoured.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
fail=0
for d in tests/*/; do
  if [[ -x $d/run.sh || -f $d/run.sh ]]; then bash "$d/run.sh" || fail=1
  else python3 -m unittest discover -q -s "$d" || fail=1; fi
done
for c in tools/deck tools/mmm; do (cd "$c" && cargo test --offline --quiet) || fail=1; done
exit "$fail"
