#!/usr/bin/env bash
# scripts/check-ux-scenarios.sh — ADVISORY reminder to keep docs/ux/scenarios.md in sync
# (S-UXR spec A2; repointed to the super-ux catalog in the 2026-07-24 audit remediation). If a change touches user-facing UI (src/components/**, src/App.tsx, src/store/**)
# but does NOT also update docs/qa/ux-scenarios.md in the same range, it prints a loud WARNING and
# lists the changed files.
#
# It ALWAYS exits 0 — it warns, it never blocks. A hard gate here would create false friction on
# pure-logic changes (a refactor that touches no scenario). Wired as an INFORMATIONAL stage in
# scripts/final-suite.sh and a `continue-on-error` step in .github/workflows/ci.yml (CONTRIBUTING.md
# keeps the rule + these two in lockstep).
#
# Usage: check-ux-scenarios.sh [BASE [HEAD]]   (defaults: HEAD~1 HEAD)
#   CI passes the push/PR range; local dev uses the default previous-commit range.
set -uo pipefail   # NOT -e: a transient git error must never turn this advisory into a hard failure.

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

BASE="${1:-HEAD~1}"
HEAD_REF="${2:-HEAD}"
CATALOG="docs/ux/scenarios.md"

# No comparable base (shallow clone, or the very first commit) → nothing to compare, skip quietly.
if ! git rev-parse --verify "$BASE" >/dev/null 2>&1; then
  echo "ux-scenarios: no comparable base ($BASE) — skipping advisory check."
  exit 0
fi

changed="$(git diff --name-only "$BASE" "$HEAD_REF" 2>/dev/null || true)"
ui_changed="$(printf '%s\n' "$changed" | grep -E '^(src/components/|src/App\.tsx$|src/store/)' || true)"
catalog_changed="$(printf '%s\n' "$changed" | grep -Fx "$CATALOG" || true)"

if [[ -n "$ui_changed" && -z "$catalog_changed" ]]; then
  echo "WARNING: UI files changed but $CATALOG was NOT updated in ${BASE}..${HEAD_REF}:" >&2
  printf '  %s\n' $ui_changed >&2
  echo >&2
  echo "  Per CONTRIBUTING.md (UX scenarios rule): any change to a user-facing control, view, or" >&2
  echo "  state — or a UI-consumed wire verb — must update $CATALOG in the SAME change (add/" >&2
  echo "  adjust the affected scenarios, statuses, and coverage cites)." >&2
  echo "  This is an ADVISORY reminder — it does NOT fail the build." >&2
  exit 0
fi

if [[ -n "$ui_changed" ]]; then
  echo "ux-scenarios: UI changed and $CATALOG updated in the same range — OK."
else
  echo "ux-scenarios: no user-facing UI files changed — nothing to check."
fi
exit 0
