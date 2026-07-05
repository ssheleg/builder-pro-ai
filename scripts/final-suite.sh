#!/usr/bin/env bash
# scripts/final-suite.sh — the single command that gates the whole S0+S1 Definition of Done
# (spec §14.3, Task 25). Runs, IN ORDER, and stops at the first failure with a specific message:
#
#   1. Full Rust workspace test suite (`cargo test --workspace`).
#   2. Clippy across the whole workspace with warnings denied (`-D warnings`).
#   3. Full TypeScript test suite (`npx vitest run`).
#   4. ts-rs type parity: regenerate `src/ipc/types.ts` from `crates/protocol` and diff against
#      what's committed — a diff means the generated bindings are stale (spec §5, §14.2 row 1).
#   5. Daemon-crate coverage gate (bpa-sessiond line coverage >= 80%, spec §14.3).
#   6. E2E survive-restart (spec §14.1/§13's core promise).
#
# Exits 0 and prints "ALL GATES PASSED" only if every stage succeeds.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

echo "== 1/8 Rust workspace tests =="
cargo test --workspace
echo "OK: cargo test --workspace"

echo
echo "== 2/8 clippy (deny warnings) =="
cargo clippy --workspace --all-targets -- -D warnings
echo "OK: clippy -D warnings"

echo
echo "== 3/8 rustfmt (formatting is normative) =="
cargo fmt --check
echo "OK: cargo fmt --check"

echo
echo "== 4/8 TypeScript tests =="
npx vitest run
echo "OK: npx vitest run"

echo
echo "== 5/8 TypeScript typecheck =="
npx tsc --noEmit
echo "OK: npx tsc --noEmit"

echo
echo "== 6/8 ts-rs type parity (generated types in sync) =="
# The `crates/protocol/tests/ts_export.rs` tests regenerate src/ipc/types.ts as a side effect of
# running (each test calls `export_all_to` before asserting on the content) — running them here
# both proves the export path still works AND leaves types.ts freshly regenerated for the diff
# below. A non-empty diff means someone edited crates/protocol without regenerating the TS
# mirror (or hand-edited types.ts, which is explicitly forbidden — spec §4).
cargo test -p bpa-protocol --test ts_export
git diff --exit-code -- src/ipc/types.ts || {
  echo "FAIL: src/ipc/types.ts is out of sync with crates/protocol (regenerate + commit it)"
  exit 1
}
echo "OK: src/ipc/types.ts matches crates/protocol"

echo
echo "== 7/8 daemon coverage gate (>= 80%) =="
bash "$REPO/scripts/coverage-gate.sh"

echo
echo "== 8/8 e2e survive-restart =="
cargo build -p bpa-sessiond
npm run e2e:survive
echo "OK: npm run e2e:survive"

echo
echo "ALL GATES PASSED"
