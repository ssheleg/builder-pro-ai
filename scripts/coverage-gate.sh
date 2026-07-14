#!/usr/bin/env bash
# scripts/coverage-gate.sh — daemon-crate line coverage gate (Task 25, spec §14.3 DoD:
# "daemon-crate line coverage ≥ 80%").
#
# Requires `cargo-llvm-cov` (https://github.com/taiki-e/cargo-llvm-cov):
#   rustup component add llvm-tools-preview
#   cargo install cargo-llvm-cov
#
# This is a REAL gate, not a report: `cargo llvm-cov --fail-under-lines 80` exits non-zero if
# coverage drops below the threshold, and this script propagates that exit code (`set -euo
# pipefail` — the `cargo llvm-cov` failure aborts the script before the "OK" line prints).
#
# Disk note (Task 25): coverage instrumentation builds an LLVM-instrumented profile of the
# `sessiond` crate and its dependency tree ALONGSIDE the normal `dev`/`test` profiles already in
# `target/` — budget roughly another full debug build's worth of disk (the daemon crate's heavier
# deps are `alacritty_terminal`, `portable-pty`, `rusqlite`, `tokio`). On a disk with only a few
# GB free this can be the difference between "fits" and "No space left on device" — check `df -h`
# before running this on a constrained box; see docs/traceability.md for how this task evidenced
# coverage without running the instrumented build.
#
# S3 (Task 20, spec §12): gains a second daemon-crate gate for `bpa-orchd` (line coverage >= 80%,
# same bar as sessiond — no lower threshold for the newer daemon). Both gates run in the same
# script/CI job so a shortfall in EITHER daemon fails this one gate, never silently passes on one
# crate's coverage alone.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

command -v cargo-llvm-cov >/dev/null 2>&1 || {
  echo "FAIL: cargo-llvm-cov not installed."
  echo "  Install with: rustup component add llvm-tools-preview && cargo install cargo-llvm-cov"
  exit 1
}

cd "$REPO"
echo "== daemon-crate (bpa-sessiond) line coverage (>= 80%) =="
cargo llvm-cov --package bpa-sessiond --fail-under-lines 80
echo "OK: bpa-sessiond coverage >= 80%"

echo
echo "== daemon-crate (bpa-orchd) line coverage (>= 80%) =="
cargo llvm-cov --package bpa-orchd --fail-under-lines 80
echo "OK: bpa-orchd coverage >= 80%"
