#!/usr/bin/env bash
# Test for check-ux-scenarios.sh (S-UXR A2) — proves the advisory warns when UI changed without the
# catalog, is silent when both changed, and ALWAYS exits 0. Self-contained (builds a temp git repo).
# Exits 0 on pass, 1 on failure.
set -uo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$SELF_DIR/check-ux-scenarios.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cd "$TMP"
git init -q
git config user.email t@example.com
git config user.name tester
mkdir -p scripts src/components docs/qa
cp "$SCRIPT" scripts/check-ux-scenarios.sh
echo "catalog v0" > docs/qa/ux-scenarios.md
echo "export const A = 1;" > src/components/A.tsx
git add -A && git commit -qm base

fail() { echo "FAIL: $1"; exit 1; }

# Case 1: a component changed, the catalog did NOT → WARNING + exit 0.
echo "// edit" >> src/components/A.tsx
git add -A && git commit -qm c1
out="$(bash scripts/check-ux-scenarios.sh HEAD~1 HEAD 2>&1)"; rc=$?
[ "$rc" -eq 0 ] || fail "case1: advisory must exit 0, got $rc"
printf '%s' "$out" | grep -q "WARNING" || fail "case1: expected a WARNING, got: $out"

# Case 2: both the component AND the catalog changed → OK, no WARNING.
echo "// edit2" >> src/components/A.tsx
echo "catalog v1" >> docs/qa/ux-scenarios.md
git add -A && git commit -qm c2
out="$(bash scripts/check-ux-scenarios.sh HEAD~1 HEAD 2>&1)"; rc=$?
[ "$rc" -eq 0 ] || fail "case2: exit 0"
printf '%s' "$out" | grep -q "WARNING" && fail "case2: unexpected WARNING when both changed"

# Case 3: no UI files changed (docs-only) → silent OK, exit 0.
echo "note" >> docs/qa/ux-scenarios.md
git add -A && git commit -qm c3
out="$(bash scripts/check-ux-scenarios.sh HEAD~1 HEAD 2>&1)"; rc=$?
[ "$rc" -eq 0 ] || fail "case3: exit 0"
printf '%s' "$out" | grep -q "WARNING" && fail "case3: unexpected WARNING on docs-only change"

echo "check-ux-scenarios.test: PASS"
