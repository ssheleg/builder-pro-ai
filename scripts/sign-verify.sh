#!/usr/bin/env bash
# scripts/sign-verify.sh — verify a universal .app produced by scripts/build-universal.sh
# (Task 24; spec §14.3 DoD packaging gate).
#
# Three checks, in order:
#   1. `codesign --verify --deep --strict` on the .app — the whole bundle, including BOTH
#      embedded daemon sidecars (bpa-sessiond AND bpa-orchd, S3), must carry a valid, complete
#      signature.
#   2. Each embedded sidecar's OWN signature is checked directly (belt-and-suspenders: `--deep`
#      above already covers them, but a standalone check gives a clearer failure message
#      pinpointing the specific sidecar if something in the deep-sign step is broken).
#   3. `spctl --assess --type execute` — the real Gatekeeper policy check. This is the one that
#      actually distinguishes "signed" from "signed AND notarized": a dev-signed (or ad-hoc)
#      build fails this even though codesign --verify passes, because spctl also requires a
#      stapled/online-verifiable notarization ticket.
#
# Honest degradation: if this machine's build was produced without notarization creds (see
# scripts/build-universal.sh), an spctl rejection here is EXPECTED, not a failure — this script
# still exits 0 in that case, but never claims the artifact IS notarized when it inspects a
# rejection. It only treats spctl rejection as a hard failure when notarization creds WERE
# present for the build (i.e. the artifact was supposed to be the real, distributable thing).

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-$REPO/src-tauri/target/universal-apple-darwin/release/bundle/macos/Builder Pro AI.app}"

log()  { echo "[sign-verify] $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

[ -d "$APP" ] || fail "app bundle not found at: $APP (run scripts/build-universal.sh first, or pass the .app path as \$1)"

echo "== codesign --verify --deep --strict =="
codesign --verify --deep --strict --verbose=2 "$APP" \
  || fail "deep signature verification failed on: $APP"
log "OK: codesign --verify --deep --strict passed"

echo "== embedded sidecar signatures =="
for daemon in bpa-sessiond bpa-orchd; do
  SIDECAR="$APP/Contents/MacOS/$daemon"
  if [ ! -f "$SIDECAR" ]; then
    SIDECAR="$(/usr/bin/find "$APP/Contents" -name "$daemon*" -type f | head -1)"
  fi
  [ -n "${SIDECAR:-}" ] && [ -f "$SIDECAR" ] || fail "embedded $daemon not found anywhere under $APP/Contents"
  codesign --verify --strict --verbose=2 "$SIDECAR" || fail "sidecar at $SIDECAR is not signed / signature invalid"
  log "OK: sidecar signed at $SIDECAR"
done

echo "== spctl --assess (Gatekeeper) =="
SPCTL_OK=0
spctl --assess --type execute --verbose=4 "$APP" && SPCTL_OK=1 || SPCTL_OK=0

# Recompute "were notarization creds present" independently of build-universal.sh's own process
# (this script may run as a separate invocation/CI step) — same two accepted credential sets.
NOTARIZED_EXPECTED=0
if [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
  NOTARIZED_EXPECTED=1
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARIZED_EXPECTED=1
fi

if [ "$SPCTL_OK" -eq 1 ]; then
  log "OK: spctl accepted (notarized / valid Developer ID signature)"
elif [ "$NOTARIZED_EXPECTED" -eq 0 ]; then
  log "EXPECTED-REJECT: dev-signed path (no notarization creds in this shell) — spctl rejected;"
  log "  this is NOT a failure for a dev build, but this artifact must not be distributed."
else
  fail "spctl rejected a build that WAS supposed to be notarized (notarization creds were present) — check that 'tauri build' actually stapled the ticket (see docs/build-macos.md)"
fi

echo "== first-launch smoke note =="
log "sign-verify does not itself launch the app. For the full first-launch"
log "create-terminal -> quit -> relaunch -> reattach smoke test, run:"
log "  bash scripts/smoke-clean-vm.sh \"$APP\""
log "on a clean macOS VM/machine (see docs/build-macos.md)."

log "OK: sign-verify complete"
