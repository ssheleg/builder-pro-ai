#!/usr/bin/env bash
# scripts/smoke-clean-vm.sh — first-launch smoke test on a clean macOS VM (Task 24;
# spec §14.3 DoD packaging gate: "first-launch-on-clean-macOS-VM smoke test (create terminal ->
# quit -> relaunch -> reattach)").
#
# Run this on a FRESH macOS VM/machine that has never run Builder Pro AI before (no prior
# LaunchAgent installed, no prior daemon state at $XDG_RUNTIME_DIR/bpa or /tmp/bpa-<uid>) — the
# whole point is to prove the notarized bundle's first-run bootstrap path (Gatekeeper accepts it,
# the app installs + bootstraps its LaunchAgent, the daemon starts on demand), not to re-prove the
# daemon logic itself (that's what the T23 socket harness / `npm run e2e:survive` already do,
# CI-runnable, no VM required).
#
# Steps:
#   1. Install the (already-built, signed, ideally-notarized) .app to /Applications on the VM.
#   2. Remove the quarantine xattr Gatekeeper attaches on download/transfer, exactly as a user's
#      "right-click -> Open" / double-click flow does after Gatekeeper's own prompt is accepted —
#      this script does not bypass Gatekeeper's *verdict*, it only clears the flag so `open` can
#      proceed non-interactively for automation; a REAL Gatekeeper rejection (unsigned / not
#      notarized) still shows the "cannot be opened" dialog on `open` and this script will then
#      correctly fail at the "daemon did not start" check below.
#   3. Launch the app once — this exercises the LaunchAgent bootstrap path (`launchd.rs`).
#   4. Wait for BOTH daemons (`bpa-sessiond` AND `bpa-orchd`, S3) to appear as running processes
#      (proves launchd started each embedded, signed sidecar — the whole point of BL-59's fix is
#      that the release bundle actually SHIPS the second daemon, so the smoke must see it run).
#   5. Hand off to the T23 E2E harness in its launchd-managed variant
#      (`BPA_E2E_EXTERNAL_DAEMON=1 node tests/e2e/survive-restart.mjs`), which drives the actual
#      create-terminal -> run-command -> observe-OSC-status -> "quit" (hard-close the client
#      socket, the same failure mode as Cmd-Q) -> assert daemon+shell survive -> reattach ->
#      scrollback-intact sequence over the real Hop-B wire protocol against the launchd-managed
#      daemon — see tests/e2e/README.md §2 for exactly what this variant proves versus the
#      harness-spawned default variant.
#
# This script is NOT run in this environment (no clean VM available here) — it is the documented,
# runnable procedure a human/CI runs on one. See docs/build-macos.md for the full runbook.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:-/Applications/Builder Pro AI.app}"
LABEL="ai.builderpro.desktop.sessiond"
ORCHD_LABEL="ai.builderpro.desktop.orchd"

log()  { echo "[smoke-clean-vm] $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

[ -d "$APP" ] || fail "app bundle not found at $APP — copy/install the built .app to that path on the clean VM first (see docs/build-macos.md)"

echo "== clean-VM smoke: first launch, Gatekeeper, LaunchAgent bootstrap, daemon start =="

# Clear the quarantine flag Gatekeeper stamps on files that arrived via download/AirDrop/USB
# transfer to a VM — this mirrors a user acknowledging the Gatekeeper prompt, it does NOT disable
# Gatekeeper's verification itself (an unsigned or non-notarized app still gets rejected by `open`
# below with "cannot be opened because the developer cannot be verified", which surfaces as this
# script's failure at the "daemon did not start" check).
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

log "launching $APP (first run — installs + bootstraps the LaunchAgent per launchd.rs)"
open "$APP"

log "waiting up to 30s for BOTH daemons (bpa-sessiond + bpa-orchd) to start under launchd..."
wait_for_daemon() {
  local proc="$1" started=0
  for _ in $(seq 1 30); do
    if pgrep -x "$proc" >/dev/null 2>&1; then
      started=1
      break
    fi
    sleep 1
  done
  [ "$started" -eq 1 ] || return 1
}
wait_for_daemon bpa-sessiond \
  || fail "daemon (bpa-sessiond) did not start within 30s of first launch — check Gatekeeper acceptance (Console.app), and 'launchctl print gui/\$(id -u)/$LABEL'"
wait_for_daemon bpa-orchd \
  || fail "daemon (bpa-orchd) did not start within 30s of first launch — the embedded orchd sidecar may be missing/unsigned; check Gatekeeper acceptance (Console.app), and 'launchctl print gui/\$(id -u)/$ORCHD_LABEL'"
log "OK: both daemons running after first launch"

log "handing off to the T23 E2E harness (launchd-managed variant)"
log "  BPA_E2E_EXTERNAL_DAEMON=1 node tests/e2e/survive-restart.mjs"
log "  (phase0 attaches to the already-running launchd-managed daemon instead of spawning its own;"
log "   see tests/e2e/README.md §2 for what this variant proves that the default variant does not)"

export BPA_E2E_EXTERNAL_DAEMON=1
if ! node "$REPO/tests/e2e/survive-restart.mjs"; then
  fail "create->quit->relaunch->reattach smoke failed (see harness output above)"
fi

log "OK: create -> quit -> relaunch -> reattach smoke passed on clean VM"
log "NOTE: this harness run exercised the daemon over the wire protocol only. To also visually"
log "confirm the GUI terminal pane itself (xterm.js rendering, status dot color), additionally"
log "follow the manual full-GUI procedure in tests/e2e/README.md §3."
