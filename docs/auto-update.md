# Auto-update (Tauri v2 Updater)

> Added in `0.10.2`. From `0.10.2` onward, a running install **auto-updates itself** from GitHub
> Releases: on launch it checks a manifest, verifies the update against an embedded public key,
> downloads, installs, and relaunches — with all data, settings, and Keychain secrets preserved.

## How it works (the chain)

1. **Check.** On app launch (`src/App.tsx` → `src/updater.ts`), the Tauri updater plugin GETs the
   configured endpoint:
   `https://github.com/ssheleg/builder-pro-ai/releases/latest/download/latest.json`. The manifest
   declares the newest `version`, its `pub_date`, and per-platform `{signature, url}` entries.
2. **Compare.** If the manifest's `version` is greater than the running app's, an update is offered
   (a confirm dialog — honest about the fact that running terminals restart once).
3. **Download + verify.** On accept, the plugin downloads the `.app.tar.gz` (the universal macOS
   updater bundle) and verifies its detached minisign signature against the **public key embedded in
   `src-tauri/tauri.conf.json > plugins.updater.pubkey`**. A tampered asset — even one uploaded to a
   Release — is rejected. Signature verification cannot be disabled.
4. **Install + relaunch.** `update.downloadAndInstall()` swaps the `.app` bundle, then
   `relaunch()` (from `@tauri-apps/plugin-process`) restarts the app.

## Data preservation — automatic by design

Nothing user-owned lives inside the `.app` bundle, so swapping it preserves everything:

| Data | Location | Survives? |
|------|----------|-----------|
| Domain (projects/goals/ideas/insights/tasks/graph/MCP/trust) | `orchd.db` in `app_data_dir` | ✅ outside `.app` |
| Terminals (scrollback, command-events, lifecycle) | `bpa.db` in `app_data_dir` | ✅ outside `.app` |
| UI preferences (theme, keep-awake) | webview `localStorage` | ✅ outside `.app` |
| Secrets (bearer tokens, API keys) | macOS Keychain | ✅ keyed by service/account |
| launchd agents | `~/Library/LaunchAgents/*.plist` | ✅ rewritten on boot |

The **DB schema migrates forward** on the reloaded daemon's boot — `orchd.db`/`bpa.db` use
additive, forward-only, single-transaction migrations (`PRAGMA user_version`), so an older store is
upgraded in place when the new daemon opens it.

### The one interruption: live terminals

After install + relaunch, the new GUI reloads both `launchd` daemons from the new `.app` (see
`reconcile_daemon_version` below), which restarts running shells. Their **history/scrollback
persists in `bpa.db` and re-attaches**; only the live *running* processes are interrupted. The
update dialog states this honestly.

## Daemon reconcile (`reconcile_daemon_version`, `src-tauri/src/lib.rs`)

When the updater swaps the `.app` and relaunches the GUI, `launchd` is **still running the OLD
daemon binary** — REL-1 deliberately made `bootstrap()` treat "already bootstrapped" as success
with **no bootout** (so a normal launch never kills live sessions), and the plain `kickstart()` is
non-force (never reloads a running daemon). Without a reconcile, a new app version would run its new
GUI against the OLD daemons — daemon fixes would never take effect.

`reconcile_daemon_version` fixes this, gated by a persisted version marker per daemon
(`app_data_dir/daemon-applied-{sessiond,orchd}.txt`):

- **marker == current version** → the daemon binary last loaded is current → no-op (the normal
  launch path — no terminal disruption).
- **marker absent (first-ever launch)** → the daemon is being bootstrapped fresh from the current
  `.app` by `ensure_daemon_running`, so it is already current → seed the marker, no force-reload.
- **marker != current (just auto-updated)** → `kickstart_force` (`-k`) kills the old daemon and
  relaunches it from the new `.app` → advance the marker. Best-effort (logged, never fatal).

## Signing setup (one-time)

1. Generate a minisign keypair (already done; the public key is in `tauri.conf.json`):
   ```
   npx @tauri-apps/cli signer generate -w ~/.tauri/builder-pro-ai-updater.key -p "" --ci
   ```
2. The **public key** (`…key.pub` contents) is committed in
   `src-tauri/tauri.conf.json > plugins.updater.pubkey`.
3. The **private key** (`…key` contents) is the repo secret **`TAURI_SIGNING_PRIVATE_KEY`** (set
   via `gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/builder-pro-ai-updater.key`). It is
   never committed. If a password was set, also add `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

> ⚠️ If you lose the private key, you cannot sign new updates and existing installs can no longer
> auto-update. Back it up.

## Cutting a release (the owner-triggered flow)

Releases are **manual** (`.github/workflows/release.yml`, `workflow_dispatch` — macOS runners bill
at 10× and a signed build needs the Apple + Tauri secrets). The flow:

1. Merge the work to `main` (releases are cut only from `main` — `docs/branching.md` rule 4).
2. Bump the version in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`.
3. Trigger the workflow:
   ```
   gh workflow run release.yml --ref main -f version=<version>
   ```
4. The workflow: builds the universal (arm64 + x86_64) `.app`/`.dmg`, signs + notarizes it (Apple
   credentials), **and** — because `bundle.createUpdaterArtifacts=true` and
   `TAURI_SIGNING_PRIVATE_KEY` is set — emits the signed `.app.tar.gz` + `.app.tar.gz.sig`, then
   generates `latest.json` (both `darwin-{aarch64,x86_64}` → the one universal bundle) and uploads
   all of them to the Release.
5. The Release is created as a **draft** — review it (optionally `scripts/sign-verify.sh` + a
   clean-VM smoke), then publish:
   ```
   gh release edit v<version> --draft=false --prerelease=false
   ```
6. Once published, it becomes "latest", so `releases/latest/download/latest.json` resolves to it —
   and every running `0.10.2+` install auto-updates on its next launch.

## Endpoint + visibility

The endpoint is `releases/latest/download/latest.json`, which resolves to the **latest
non-draft, non-prerelease** Release's `latest.json` asset. The repo must be **public** for an
unauthenticated client (the running app) to fetch it — a private repo's asset URLs return 404. (For
a private distribution, host `latest.json` + the `.app.tar.gz` on a public CDN and repoint
`plugins.updater.endpoints`.)

## Verifying an update artifact

```sh
gh release download v<version> --pattern '*.dmg'
# mount + Gatekeeper check (the .app inside is the notarized unit):
hdiutil attach Builder-Pro-AI-<version>-universal.dmg -nobrowse -readonly
spctl -a -t exec -vvv "/Volumes/Builder Pro AI/Builder Pro AI.app"   # → accepted, Notarized Developer ID
xcrun stapler validate "/Volumes/Builder Pro AI/Builder Pro AI.app"  # → worked
```
