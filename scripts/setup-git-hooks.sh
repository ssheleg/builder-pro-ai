#!/usr/bin/env bash
# Install this repo's git hooks into .git/hooks (per-clone; run once after cloning). Currently:
# a pre-push guard that protects `main` from force-push and deletion (local stand-in for GitHub
# branch protection, which needs Pro / a public repo). See docs/branching.md.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install -m 0755 "$REPO/scripts/git-hooks/pre-push" "$REPO/.git/hooks/pre-push"
echo "OK: installed pre-push hook -> .git/hooks/pre-push (protects main: no force-push, no delete)."
