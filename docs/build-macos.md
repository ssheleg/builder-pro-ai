# Building, signing, and notarizing the macOS universal app

This is the full runbook for producing a distributable, notarized universal (Apple
Silicon + Intel) `.app`/`.dmg` for Builder Pro AI, and the honest degradation path
when Apple Developer credentials aren't available. It implements the packaging gate
in spec §14.3 (Definition of Done) and the sidecar deep-signing contract in §8.3/§16
of `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`.

Three scripts, run in order:

```sh
bash scripts/build-universal.sh    # build daemon (both arches) + universal .app, sign, notarize
bash scripts/sign-verify.sh        # verify the signature + Gatekeeper acceptance
bash scripts/smoke-clean-vm.sh     # (on a CLEAN macOS VM only) first-launch create/quit/reattach
```

**Important — this is a human/CI step, not something run in an unattended assistant
session.** It needs (a) real Apple Developer Program credentials the assistant does
not have and must never be asked to fabricate, and (b) a full release build
(`cargo build --release` x2 + `tauri build`), which is disk- and time-heavy. The
sections below are written for whoever (a person, or a CI runner with secrets
configured) actually executes the pipeline.

---

## 1. Prerequisites

- **macOS**, with Xcode command line tools installed (`xcode-select -p` should print
  a path; `codesign`, `spctl`, `xcrun` come from this).
- **Rust toolchain** with both macOS targets:
  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  ```
- **Node.js + npm** with project deps installed (`npm install`).
- **Disk space:** a full universal release build (two `cargo build --release`
  daemon builds + a universal `tauri build`, which itself does a release build of
  the Tauri core for both arches and `lipo`-merges them) needs on the order of
  **10-15 GB free** in `target/` across `crates/sessiond` and `src-tauri`, plus
  bundle output. Do not attempt this on a machine with only a few GB free — it was
  explicitly NOT run in the environment that authored these scripts for exactly
  this reason (see "Why this wasn't run here" below).
- **An Apple Developer Program membership** (paid, $99/yr) for a Developer ID
  Application certificate. Notarization is impossible without one.

---

## 2. Apple Developer account setup (one-time, human step)

1. Enroll in the [Apple Developer Program](https://developer.apple.com/programs/)
   if not already enrolled.
2. **Create a Developer ID Application certificate:**
   Xcode → Settings → Accounts → your team → Manage Certificates → "+" →
   "Developer ID Application". (Or via the [developer
   portal](https://developer.apple.com/account/resources/certificates/list).)
   This installs the certificate + private key into your login keychain.
3. **Find your signing identity string:**
   ```sh
   security find-identity -v -p codesigning
   ```
   Look for a line like:
   ```
   1) ABCD1234... "Developer ID Application: Your Name / Company (TEAMID)"
   ```
   The quoted string is `APPLE_SIGNING_IDENTITY`. The parenthesized 10-character
   code is `APPLE_TEAM_ID`.
4. **Create an App Store Connect API key** (preferred notarization method — no
   interactive Apple ID 2FA prompts, works headlessly in CI):
   - [App Store Connect](https://appstoreconnect.apple.com/) → Users and Access →
     Integrations tab → "+" → give it a name, role **Developer** or higher (Admin
     works too).
   - Note the **Issuer ID** shown above the keys table → `APPLE_API_ISSUER`.
   - Note the **Key ID** for the new key → `APPLE_API_KEY`.
   - Download the private key (`AuthKey_<KEY_ID>.p8`) — **this is only downloadable
     once**, immediately after creation. Store it somewhere durable and secret
     (a CI secrets store, a local encrypted location — never commit it to git).
     Its absolute path is `APPLE_API_KEY_PATH`.
   - Alternative (not preferred — requires an app-specific password and ties
     notarization to a personal Apple ID rather than a team-scoped API key): set
     `APPLE_ID` (your Apple ID email) and `APPLE_PASSWORD` (an app-specific
     password generated at https://appleid.apple.com/account/manage, NOT your
     normal Apple ID password) instead of the three `APPLE_API_*` vars.

---

## 3. Env-var contract

| Variable | Required for | Meaning |
|---|---|---|
| `APPLE_SIGNING_IDENTITY` | Real (non-ad-hoc) code signing | `Developer ID Application: Name (TEAMID)` string from `security find-identity -v -p codesigning`. |
| `APPLE_TEAM_ID` | Signing + notarization | 10-character Apple Developer Team ID. |
| `APPLE_API_ISSUER` | Notarization (API key path, preferred) | App Store Connect Issuer ID. |
| `APPLE_API_KEY` | Notarization (API key path, preferred) | App Store Connect API Key ID. |
| `APPLE_API_KEY_PATH` | Notarization (API key path, preferred) | Absolute path to the downloaded `AuthKey_<KEY_ID>.p8`. |
| `APPLE_ID` | Notarization (Apple ID path, alternative) | Apple ID email. |
| `APPLE_PASSWORD` | Notarization (Apple ID path, alternative) | App-specific password (not your Apple ID password). |

`scripts/build-universal.sh` accepts **either** the `APPLE_API_*` triple **or** the
`APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID` triple as "notarization creds present";
the App Store Connect API key path is preferred (no interactive 2FA, cleanly
scoped, works in CI).

None of these are hardcoded anywhere in this repo (not in `tauri.conf.json`, not in
the scripts) — they are read from the environment only, at build time. This is
deliberate: a missing/empty env var must never be silently treated as "use some
default identity" that could produce a misleadingly-labeled artifact.

Tauri v2's bundler (`tauri build`) reads these variables itself during the macOS
bundle step: with `APPLE_SIGNING_IDENTITY`/`APPLE_TEAM_ID` set it deep-signs the
`.app` — including every embedded binary the OS treats as a nested code object,
which for us means the embedded `bpa-sessiond` sidecar gets the app's hardened
runtime entitlements and a Developer ID signature too (`codesign --deep`
semantics). With the `APPLE_API_*` (or `APPLE_ID`/`APPLE_PASSWORD`) set on top of
that, `tauri build` additionally uploads the signed `.app` to Apple's notary
service, polls for the result, and staples the notarization ticket to the bundle —
all automatically, as part of the same `tauri build` invocation. No separate
`xcrun notarytool` invocation is needed in the common case.

---

## 4. The build → sign → notarize → verify sequence

```sh
export PATH="$HOME/.cargo/bin:$PATH"

APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
APPLE_TEAM_ID="TEAMID" \
APPLE_API_ISSUER="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" \
APPLE_API_KEY="ABCD1234EF" \
APPLE_API_KEY_PATH="/absolute/secure/path/AuthKey_ABCD1234EF.p8" \
bash scripts/build-universal.sh

bash scripts/sign-verify.sh
```

What each step does:

1. **`scripts/build-universal.sh`**
   - Checks prereqs (`rustup`, `cargo`, `npm`, both darwin targets installed); run
     `bash scripts/build-universal.sh --check-prereqs` on its own any time to just
     run this check without building anything.
   - `cargo build -p bpa-sessiond --release --target aarch64-apple-darwin`
   - `cargo build -p bpa-sessiond --release --target x86_64-apple-darwin`
   - Copies the two resulting binaries into `src-tauri/binaries/` with Tauri's
     required target-triple-suffixed names:
     `bpa-sessiond-aarch64-apple-darwin`, `bpa-sessiond-x86_64-apple-darwin`
     (`tauri.conf.json`'s `bundle.externalBin` references the un-suffixed
     `binaries/bpa-sessiond` — Tauri appends the triple itself when it looks up
     which file to embed for the arch it's building).
   - Runs `npm run tauri -- build --target universal-apple-darwin`, which:
     - `lipo`-merges the two sidecar binaries and the app's own Rust binary into a
       single universal Mach-O per binary,
     - produces `.app` and `.dmg` bundles (per `tauri.conf.json`'s
       `bundle.targets`),
     - deep-signs everything with `APPLE_SIGNING_IDENTITY` (hardened runtime +
       `src-tauri/entitlements.plist`, per `tauri.conf.json`'s `bundle.macOS`
       config),
     - if the notarization env vars are present, uploads to Apple's notary
       service, polls until a result comes back, and staples the ticket to the
       `.app` (and `.dmg`, if built) — all inside this one `tauri build` call.
   - **If credentials are absent** (either `APPLE_SIGNING_IDENTITY` or the
     notarization set): prints a loud `WARNING:` to stderr, proceeds to build a
     dev-signed / ad-hoc-signed artifact anyway, and exits **0** — see §6 below.

2. **`scripts/sign-verify.sh [path-to-.app]`** (defaults to the path
   `tauri build --target universal-apple-darwin` produces)
   - `codesign --verify --deep --strict --verbose=2 <app>` — the whole bundle,
     including the embedded sidecar, must have a complete, valid signature.
   - Locates the embedded `bpa-sessiond` inside `<app>/Contents/` and verifies its
     signature individually (a clearer failure message than relying solely on
     `--deep` if something about the sidecar's signing specifically is broken).
   - `spctl --assess --type execute --verbose=4 <app>` — the actual Gatekeeper
     policy check. This is the one that distinguishes a merely-signed build from a
     notarized one: `codesign --verify` can pass on an ad-hoc/dev-signed build,
     but `spctl` additionally requires a stapled, verifiable notarization ticket
     (or a plain Developer-ID signature Apple's servers can verify online) to
     accept it.
   - Prints a reminder to run `scripts/smoke-clean-vm.sh` next.

3. **`scripts/smoke-clean-vm.sh [path-to-installed-.app]`** — see §5.

---

## 5. Clean-VM first-launch smoke procedure (human step)

This is the highest-fidelity check in the pipeline and the one this environment
genuinely cannot run — it requires a real (or freshly-snapshotted) macOS VM that
has never run Builder Pro AI before, so the LaunchAgent bootstrap and Gatekeeper's
"first launch of a downloaded app" behavior are exercised for real, not skipped
because the state already exists from a previous run.

**Setup (one-time per VM image):**
1. A clean macOS VM (e.g. a fresh VM/snapshot in Tart, UTM, VMware Fusion, or a
   throwaway machine) with no prior Builder Pro AI install, no
   `~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`, and no leftover
   state at `$XDG_RUNTIME_DIR/bpa` or `/tmp/bpa-<uid>`.
2. Transfer the built, signed `.app` (or the `.dmg`) to the VM — via a method that
   sets the quarantine xattr the way a real download would (AirDrop, `curl`
   download inside the VM, mounting a shared folder configured to quarantine, or
   `xattr -w com.apple.quarantine` manually if your transfer method doesn't set it
   automatically — the goal is to exercise the SAME Gatekeeper path a real user's
   download hits).
3. Copy/drag the `.app` into `/Applications`.

**Run the smoke test:**
```sh
bash scripts/smoke-clean-vm.sh "/Applications/Builder Pro AI.app"
```

This script:
1. Clears the quarantine xattr (mirrors a user accepting the Gatekeeper "are you
   sure you want to open this app downloaded from the internet" prompt — it does
   **not** bypass Gatekeeper's actual verdict; an unsigned/non-notarized `.app`
   still gets rejected by `open` and the script fails at the next step).
2. Launches the app once (`open`), which exercises the first-run LaunchAgent
   bootstrap in `src-tauri/src/launchd.rs` (writes
   `~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`,
   `launchctl bootstrap gui/$UID <plist>`).
3. Polls for up to 30s for `bpa-sessiond` to appear as a running process (proves
   launchd actually started the daemon, not just that the plist was written).
4. Hands off to the T23 E2E harness's **launchd-managed variant**:
   ```sh
   BPA_E2E_EXTERNAL_DAEMON=1 node tests/e2e/survive-restart.mjs
   ```
   which drives the real create-terminal → run-command → observe-OSC-status →
   "quit" (hard-close the client socket, the same failure mode as Cmd-Q) →
   assert-daemon-and-shell-survive → reattach → scrollback-intact sequence over
   the actual Hop-B wire protocol, against the launchd-supervised daemon (not one
   the harness spawned itself) — proving `launchd.rs`'s bootstrap/kickstart wiring
   is what's keeping the daemon reachable across a real app quit, not just the
   daemon's own in-process client-disconnect handling. See
   `tests/e2e/README.md` §2 for full detail on what this variant proves.

For a full visual confirmation of the terminal UI itself (xterm.js rendering, the
status dot's color transitions), also follow the manual GUI procedure in
`tests/e2e/README.md` §3 (launch the app, create a workspace + terminal, run a
command, watch the status dot, quit, relaunch, confirm scrollback repaints).

---

## 6. Honest degradation: no Apple credentials available

If `APPLE_SIGNING_IDENTITY` and/or the notarization credential set are not present
in the environment, `scripts/build-universal.sh` does **not** fail and does **not**
hang waiting for credentials that were never going to arrive. Instead:

- It prints explicit `WARNING:` lines to stderr identifying exactly what's missing
  and what the consequence is.
- It still builds and produces a `.app` — ad-hoc signed (`codesign` identity `-`)
  if `APPLE_SIGNING_IDENTITY` is also unset, or Developer-ID-signed-but-not-
  notarized if only the notarization creds are missing.
- It exits **0** — this is a legitimate, if limited, dev build, not an error.

That artifact:
- **Runs fine on the machine that built it** (Gatekeeper doesn't re-check a
  locally-built, locally-run app the same way it checks a downloaded one).
- **Will be quarantined/rejected by Gatekeeper on any other Mac** — a colleague
  or CI runner trying to open it will see "cannot be opened because the developer
  cannot be verified" (ad-hoc) or a similar rejection (signed-but-not-notarized).
- **Is not fit for distribution.** Do not attach it to a release, DMG download
  page, or send it to anyone outside the machine that built it.

`scripts/sign-verify.sh` mirrors this honestly on the verification side: it still
runs `codesign --verify` (which passes even on a dev-signed build — codesign only
checks the signature is internally valid, not that it's notarized) but treats an
`spctl --assess` rejection as `EXPECTED-REJECT` (not a failure, exit 0) whenever
the notarization credential env vars aren't present in that shell. If the
credentials WERE present for the build but `spctl` still rejects, that IS treated
as a hard failure — it means notarization or stapling silently didn't work and the
artifact is falsely appearing "notarized-adjacent" when it isn't.

**Never treat a dev-signed build as shippable.** The scripts will never silently
upgrade a dev-signed artifact's status or hide the warning — if you see the
`WARNING:` block, the artifact is not notarized, full stop.

---

## 7. Why the notarized build isn't run inside an assistant session

This pipeline's scripts were authored and validated in an unattended coding-
assistant session that deliberately did **not** run the actual universal build or
notarization, because:

- **No Apple Developer credentials are available there.** Notarization requires a
  real, paid Apple Developer Program membership's certificate + API key/Apple ID —
  secrets that must never be requested from, fabricated by, or stored in an
  automated session. Skipping this is correct, not a gap to work around.
- **Disk budget.** A full `tauri build --target universal-apple-darwin` performs
  two release builds (`aarch64`+`x86_64`) of the whole Tauri core plus two release
  builds of the daemon, then `lipo`-merges and bundles — routinely 10+ GB of
  `target/` growth. The authoring environment had roughly 8.5 GB free, which is
  not a safe margin for that build.
- **No clean VM.** §5's smoke test specifically needs a pristine macOS instance
  with no prior app/daemon state — not something available inside a single
  developer/assistant working directory.

What *was* validated cheaply, without running the heavy build, in that
environment:
- `bash scripts/build-universal.sh --check-prereqs` actually run — passes (both
  darwin rustup targets are installed).
- `bash scripts/sign-verify.sh` actually run with no `.app` present — correctly
  fails with `FAIL: app bundle not found at: …` (proves the guard is real, matches
  the brief's expected pre-build behavior).
- `bash scripts/smoke-clean-vm.sh` actually run with no installed `.app` present —
  correctly fails with `FAIL: app bundle not found at /Applications/…`.
- `bash -n` syntax-checked all three scripts.
- `plutil -lint src-tauri/entitlements.plist` — passes.
- `python3 -c "import json; json.load(open('src-tauri/tauri.conf.json'))"` — valid
  JSON; confirmed `bundle.externalBin`, `bundle.macOS.entitlements`,
  `bundle.macOS.hardenedRuntime`, and the newly-added
  `bundle.macOS.minimumSystemVersion` are all present and correctly shaped.
- Confirmed against the Tauri v2 docs (Context7) that `APPLE_SIGNING_IDENTITY`,
  `APPLE_TEAM_ID`, `APPLE_API_ISSUER`/`APPLE_API_KEY`/`APPLE_API_KEY_PATH`, and the
  `binary-name-<target-triple>` sidecar naming convention are exactly right.
- Confirmed the daemon crate/package name is `bpa-sessiond` (`crates/sessiond`,
  `Cargo.toml` `[package] name = "bpa-sessiond"`) and that
  `cargo build -p bpa-sessiond --release --target <triple>` is the correct
  invocation.
- Confirmed `tests/e2e/survive-restart.mjs`'s real launchd-managed-variant env var
  is `BPA_E2E_EXTERNAL_DAEMON=1` (from Task 23, already implemented and documented
  in `tests/e2e/README.md` §2) and used that exact variable in
  `scripts/smoke-clean-vm.sh`, rather than a different name.

Running the actual notarized build, and the clean-VM smoke test, are the
**human/CI steps** described in §2-§5 above.
