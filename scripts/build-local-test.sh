#!/usr/bin/env bash
# scripts/build-local-test.sh — LOCAL TEST BUILD (host-arch, debug, UNSIGNED).
#
# This is the working-branch counterpart to scripts/build-universal.sh (the release build). It
# produces a runnable `.app` from WHATEVER branch you're on (normally `nightbuild`) so you can
# smoke a change on your own Mac — fast, single-arch, no Apple credentials, no notarization.
#
# It is deliberately NOT for distribution: the bundle is ad-hoc signed (identity "-"), so macOS
# Gatekeeper will quarantine/reject it on any other machine. Distributable builds come ONLY from
# `main` via `release.yml` → `build-universal.sh` (universal + Developer-ID signed + notarized).
# See docs/branching.md for the full branch/build model.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO/src-tauri/binaries"
cd "$REPO"

log()  { echo "[build-local-test] $*"; }
warn() { echo "WARNING: $*" >&2; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# --- Resolve the pinned Rust toolchain even when the rustup shim isn't on PATH -----------------
# Prefer `cargo` on PATH; otherwise fall back to the rust-toolchain.toml-pinned toolchain under
# ~/.rustup/toolchains (some setups have the toolchains installed without the rustup PATH shim).
if ! command -v cargo >/dev/null 2>&1; then
  pinned="$(ls -d "$HOME"/.rustup/toolchains/1.92-* 2>/dev/null | head -1)"
  if [ -n "$pinned" ] && [ -x "$pinned/bin/cargo" ]; then
    export PATH="$pinned/bin:$PATH"
    log "cargo not on PATH — using pinned toolchain at $pinned/bin"
  fi
fi
command -v cargo >/dev/null 2>&1 || fail "cargo not found (install via https://rustup.rs, or ensure ~/.rustup/toolchains/1.92-* exists)"
command -v rustc >/dev/null 2>&1 || fail "rustc not found"
command -v npm   >/dev/null 2>&1 || fail "npm not found (Node >= 24)"

HOST_TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
[ -n "$HOST_TRIPLE" ] || fail "could not determine host target triple from rustc -vV"

echo
echo "================================================================================"
echo "  LOCAL TEST BUILD — $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?') @ $(git rev-parse --short HEAD 2>/dev/null || echo '?')"
echo "  host: $HOST_TRIPLE   |   UNSIGNED — not for distribution (releases come from main)"
echo "================================================================================"
echo

# --- 1. Build + stage the two daemon sidecars (debug) for the host arch -------------------------
# build.rs requires BOTH sidecars present at binaries/<name>-<host-triple> before the tauri build.
log "building daemons (debug): bpa-sessiond + bpa-orchd"
cargo build -p bpa-sessiond -p bpa-orchd
mkdir -p "$BIN_DIR"
cp "target/debug/bpa-sessiond" "$BIN_DIR/bpa-sessiond-$HOST_TRIPLE"
cp "target/debug/bpa-orchd"    "$BIN_DIR/bpa-orchd-$HOST_TRIPLE"
log "staged sidecars → $BIN_DIR/{bpa-sessiond,bpa-orchd}-$HOST_TRIPLE"

# --- 2. Frontend deps (only if missing) ---------------------------------------------------------
[ -d node_modules ] || { log "installing frontend deps (npm ci)"; npm ci; }

# --- 3. Debug .app bundle (no signing env => ad-hoc "-" signature; --bundles app skips the dmg) --
log "tauri build --debug (host arch, app bundle only)"
npm run tauri -- build --debug --bundles app

# --- 4. Report where it landed ------------------------------------------------------------------
APP="$(find target -type d -name '*.app' -path '*debug*' 2>/dev/null | head -1)"
echo
if [ -n "$APP" ]; then
  echo "[build-local-test] OK — local test build at:"
  echo "    $REPO/$APP"
  echo "    open \"$REPO/$APP\"   # to launch it"
else
  echo "[build-local-test] build finished — look under target/**/debug/bundle/macos/*.app"
fi
echo "[build-local-test] reminder: UNSIGNED. Distributable builds come from main (release.yml)."
