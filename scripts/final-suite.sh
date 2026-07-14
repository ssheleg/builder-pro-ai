#!/usr/bin/env bash
# scripts/final-suite.sh — the single command that gates the whole S0+S1+S3 Definition of Done
# (spec §14.3 [S1], S3 spec §12, Task 25 / Task 20). Runs, IN ORDER, and stops at the first
# failure with a specific message:
#
#   1. Full Rust workspace test suite (`cargo test --workspace`).
#   2. Clippy across the whole workspace with warnings denied (`-D warnings`).
#   3. rustfmt check (`cargo fmt --check` — formatting is normative).
#   4. Full TypeScript test suite (`npx vitest run`).
#   5. TypeScript typecheck (`npx tsc --noEmit`).
#   6. ts-rs type parity: regenerate `src/ipc/types.ts` from `crates/protocol` AND
#      `src/ipc/orchd-types.ts` from `crates/orchd-proto`, diff both against what's committed —
#      a diff means the generated bindings are stale (spec §5, §14.2 row 1; S3 spec §4.2).
#   7. Daemon-crate coverage gate (bpa-sessiond AND bpa-orchd line coverage >= 80%, spec §14.3,
#      S3 spec §12).
#   8. E2E survive-restart (spec §14.1/§13's core promise).
#   9. E2E orchd survive-restart + export/import round-trip (S3 spec §12 — the roadmap DoD proof:
#      goals+ideas+tasks CRUD survive restart; export/import round-trips).
#
# CI (.github/workflows/ci.yml) runs the same set — keep them in lockstep (CONTRIBUTING.md).
# Exits 0 and prints "ALL GATES PASSED" only if every stage succeeds.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

echo "== 1/9 Rust workspace tests =="
cargo test --workspace
echo "OK: cargo test --workspace"

echo
echo "== 2/9 clippy (deny warnings) =="
cargo clippy --workspace --all-targets -- -D warnings
echo "OK: clippy -D warnings"

echo
echo "== 3/9 rustfmt (formatting is normative) =="
cargo fmt --check
echo "OK: cargo fmt --check"

echo
echo "== 4/9 TypeScript tests =="
npx vitest run
echo "OK: npx vitest run"

echo
echo "== 5/9 TypeScript typecheck =="
npx tsc --noEmit
echo "OK: npx tsc --noEmit"

echo
echo "== 6/9 ts-rs type parity (generated types in sync) =="
# The `crates/protocol/tests/ts_export.rs` and `crates/orchd-proto/tests/ts_export.rs` tests
# regenerate src/ipc/types.ts and src/ipc/orchd-types.ts (respectively) as a side effect of
# running (each test calls `export_all_to` before asserting on the content) — running them here
# both proves the export path still works AND leaves both files freshly regenerated for the diffs
# below. A non-empty diff means someone edited crates/protocol or crates/orchd-proto without
# regenerating the TS mirror (or hand-edited a generated file, which is explicitly forbidden —
# spec §4, S3 spec §4.2).
cargo test -p bpa-protocol --test ts_export
git diff --exit-code -- src/ipc/types.ts || {
  echo "FAIL: src/ipc/types.ts is out of sync with crates/protocol (regenerate + commit it)"
  exit 1
}
echo "OK: src/ipc/types.ts matches crates/protocol"

cargo test -p bpa-orchd-proto --test ts_export
git diff --exit-code -- src/ipc/orchd-types.ts || {
  echo "FAIL: src/ipc/orchd-types.ts is out of sync with crates/orchd-proto (regenerate + commit it)"
  exit 1
}
echo "OK: src/ipc/orchd-types.ts matches crates/orchd-proto"

echo
echo "== 7/9 daemon coverage gate (bpa-sessiond + bpa-orchd, >= 80%) =="
bash "$REPO/scripts/coverage-gate.sh"

echo
echo "== 8/9 e2e survive-restart =="
cargo build -p bpa-sessiond
cargo build -p bpa-orchd
npm run e2e:survive
echo "OK: npm run e2e:survive"

echo
echo "== 9/9 e2e orchd survive+roundtrip =="
npm run e2e:orchd
echo "OK: npm run e2e:orchd"

echo
echo "ALL GATES PASSED"
