#!/usr/bin/env bash
# scripts/build-universal.sh — macOS universal build + deep-sign + notarize (Task 24).
#
# Pipeline (spec §14.3 DoD packaging gate, §8.3 sidecar bundling, §15.5/§16 deep-signing):
#   1. Build the `bpa-sessiond` daemon for BOTH Apple Silicon and Intel, stage them under
#      `src-tauri/binaries/` with Tauri's required target-triple-suffixed names.
#   2. Run `tauri build --target universal-apple-darwin`, which lipo-merges the frontend app
#      binary AND the two staged sidecars into a single universal `.app`/`.dmg`.
#   3. Deep-sign + notarize: Tauri's bundler does this itself, driven entirely by environment
#      variables (see the "Env-var contract" section below) — this script does not shell out to
#      `codesign`/`xcrun notarytool` directly, it just makes sure the right env vars reach
#      `tauri build`.
#
# Degradation contract (locked, spec §8.3/§14.3/§16): if the App Store Connect API key set
# (APPLE_API_ISSUER / APPLE_API_KEY / APPLE_API_KEY_PATH) is absent, this script does NOT fail
# and does NOT hang waiting on credentials that will never arrive. It prints a loud WARNING and
# proceeds to a dev-signed (or ad-hoc, if APPLE_SIGNING_IDENTITY is also unset) build. That
# artifact runs on the build machine but Gatekeeper will quarantine/reject it everywhere else —
# it is explicitly not fit for distribution. `scripts/sign-verify.sh` is the honest-degradation
# counterpart on the verification side: it treats `spctl` rejection as EXPECTED in this path,
# not a failure.
#
# ---- Env-var contract (spec §14.3 / §16; Tauri v2 macOS signing docs) ----
#
#   APPLE_SIGNING_IDENTITY   "Developer ID Application: Your Name (TEAMID)" — the codesign
#                            identity string from `security find-identity -v -p codesigning`.
#                            If unset, Tauri falls back to `tauri.conf.json > bundle > macOS >
#                            signingIdentity`, which this repo deliberately leaves UNSET so a
#                            missing env var never silently produces a falsely-labeled build —
#                            Tauri then ad-hoc signs (identity "-").
#   APPLE_TEAM_ID            Your 10-character Apple Developer Team ID. Required alongside
#                            APPLE_SIGNING_IDENTITY for a real Developer ID signature, and used
#                            as the notarytool "team" hint.
#
#   Notarization (choose ONE credential set):
#     Preferred — App Store Connect API key (non-interactive, CI-friendly):
#       APPLE_API_ISSUER     Issuer ID (App Store Connect > Users and Access > Integrations).
#       APPLE_API_KEY        Key ID (the short ID next to the key in that same table).
#       APPLE_API_KEY_PATH   Absolute path to the downloaded `AuthKey_<KEY_ID>.p8` file
#                             (downloadable exactly ONCE when the key is created — store it
#                             securely, e.g. in a CI secret store, never committed).
#     Alternative — Apple ID + app-specific password:
#       APPLE_ID              Your Apple ID email.
#       APPLE_PASSWORD         An app-specific password (NOT your normal Apple ID password) —
#                              generate one at https://appleid.apple.com/account/manage.
#       APPLE_TEAM_ID          (same var as above; required here too.)
#
# See docs/build-macos.md for the full runbook, account setup, and clean-VM smoke procedure.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO/src-tauri/binaries"
AARCH="$BIN_DIR/bpa-sessiond-aarch64-apple-darwin"
XARCH="$BIN_DIR/bpa-sessiond-x86_64-apple-darwin"

log()  { echo "[build-universal] $*"; }
warn() { echo "WARNING: $*" >&2; }
fail() { echo "FAIL: $*" >&2; exit 1; }

check_prereqs() {
  command -v rustup >/dev/null || fail "rustup not found (install via https://rustup.rs)"
  command -v cargo  >/dev/null || fail "cargo not found (install via https://rustup.rs)"
  command -v npm    >/dev/null || fail "npm not found"
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target list --installed | grep -qx "$t" \
      || fail "rust target $t not installed (rustup target add $t)"
  done
  log "OK: prereqs (rustup, cargo, npm, both darwin targets)"
}

# Build the daemon for both architectures and stage them under src-tauri/binaries/ using
# Tauri's required naming convention: "<name>-<target-triple>" (spec §8.3, confirmed against
# Tauri v2 docs "Embedding External Binaries" — externalBin references the UNsuffixed
# "binaries/bpa-sessiond" in tauri.conf.json, but the files on disk MUST carry the triple
# suffix or the bundler silently fails to find them for that arch).
build_sidecars() {
  mkdir -p "$BIN_DIR"

  log "building bpa-sessiond for aarch64-apple-darwin (release)"
  ( cd "$REPO" && cargo build -p bpa-sessiond --release --target aarch64-apple-darwin )

  log "building bpa-sessiond for x86_64-apple-darwin (release)"
  ( cd "$REPO" && cargo build -p bpa-sessiond --release --target x86_64-apple-darwin )

  cp "$REPO/target/aarch64-apple-darwin/release/bpa-sessiond" "$AARCH"
  cp "$REPO/target/x86_64-apple-darwin/release/bpa-sessiond"  "$XARCH"

  [ -f "$AARCH" ] && [ -f "$XARCH" ] || fail "missing per-arch sidecar binary after build/copy"
  # Refuse to proceed against the S0 scaffold placeholder stub (a `sh` script) — the same
  # sanity check the T23 E2E harness applies, so a stale stub can never masquerade as a real
  # signed daemon inside a shipped bundle.
  for f in "$AARCH" "$XARCH"; do
    head -c 15 "$f" | grep -q '^#!/bin/sh$' && fail "$f is still the placeholder stub — real cargo build did not overwrite it"
  done
  log "OK: both per-arch sidecars present at $BIN_DIR"
}

build_app() {
  log "running: npm run tauri -- build --target universal-apple-darwin"
  ( cd "$REPO" && npm run tauri -- build --target universal-apple-darwin )
}

notarization_creds_present() {
  [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ] \
    || { [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; }
}

dev_signed_warning() {
  warn "notarization credentials absent (need APPLE_API_ISSUER/APPLE_API_KEY/APPLE_API_KEY_PATH,"
  warn "  or APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID) — producing a DEV-SIGNED, NON-NOTARIZED artifact."
  warn "  The .app WILL run on this build machine, but Gatekeeper will quarantine/block it on any"
  warn "  other Mac (\"cannot be opened because the developer cannot be verified\" or worse)."
  warn "  This build is NOT fit for distribution. See docs/build-macos.md for the notarized runbook."
}

signing_identity_warning() {
  warn "APPLE_SIGNING_IDENTITY unset — Tauri will ad-hoc sign (identity \"-\")."
  warn "  An ad-hoc-signed .app cannot be notarized and is only runnable on this machine."
}

main() {
  case "${1:-}" in
    --check-prereqs)
      check_prereqs
      exit 0
      ;;
  esac

  check_prereqs
  build_sidecars

  local degraded=0
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    signing_identity_warning
    degraded=1
  fi
  if ! notarization_creds_present; then
    dev_signed_warning
    degraded=1
  fi

  build_app

  if [ "$degraded" -eq 1 ]; then
    warn "build complete — DEV-SIGNED / NOT NOTARIZED (see warnings above). Exiting 0 (honest degradation)."
  else
    log "OK: universal build complete — signed with $APPLE_SIGNING_IDENTITY, notarization requested."
    log "    Tauri uploads, polls, and staples the notarization ticket automatically during 'tauri build'."
  fi
  log "next: bash scripts/sign-verify.sh"
}

main "$@"
