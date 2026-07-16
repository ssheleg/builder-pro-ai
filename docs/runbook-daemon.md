# Runbook — `bpa-sessiond` daemon operations

Everything here uses REAL identifiers from the code (verified 2026-07-04). The daemon is a
per-user LaunchAgent; killing/restarting it **ends all live shells** (records + scrollback
survive and rehydrate as inactive sessions — see the survival truth table in the platform
overview §2).

## Locations

| What | Path |
|---|---|
| LaunchAgent label | `ai.builderpro.desktop.sessiond` (`src-tauri/src/launchd.rs::LABEL`) |
| Plist | `~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist` |
| Durable state root | `~/Library/Application Support/ai.builderpro.desktop/` |
| Database | `…/ai.builderpro.desktop/bpa.db` (+ `-wal`/`-shm` sidecars) |
| Logs | `…/ai.builderpro.desktop/logs/sessiond.tracing.log` |
| Socket | `$XDG_RUNTIME_DIR/bpa/d.sock`, fallback `/tmp/bpa-<uid>/d.sock` |
| Single-instance lock | `d.lock` next to the socket |
| Per-session shell-integration assets | `<socket-dir>/runtime/session-<uuid>/` (one level below the socket, under `runtime/`) |

## Inspect

```bash
# Is the LaunchAgent loaded / running?
launchctl print gui/$(id -u)/ai.builderpro.desktop.sessiond | head -30

# Is the socket alive? (refused = daemon dead, stale file possible)
ls -la ${XDG_RUNTIME_DIR:-/tmp/bpa-$(id -u)}/bpa 2>/dev/null || ls -la /tmp/bpa-$(id -u)

# Tail the daemon log
tail -f ~/Library/Application\ Support/ai.builderpro.desktop/logs/sessiond.tracing.log

# Daemon process + its shells (children of its process group)
pgrep -fl bpa-sessiond
```

## Restart

> **WARNING:** restarting the daemon ends every live shell. Session records + scrollback
> survive and rehydrate as inactive.

```bash
launchctl kickstart -k gui/$(id -u)/ai.builderpro.desktop.sessiond
```

The GUI reconnects automatically (bounded backoff) and re-hydrates the session list.

## Full reset (wipe daemon state)

```bash
launchctl bootout gui/$(id -u)/ai.builderpro.desktop.sessiond 2>/dev/null
rm -rf ~/Library/Application\ Support/ai.builderpro.desktop
rm -rf "${XDG_RUNTIME_DIR:-/tmp/bpa-$(id -u)}/bpa" /tmp/bpa-$(id -u) 2>/dev/null
# Relaunch the app — it re-installs the plist and re-bootstraps the daemon.
```

## Uninstall

```bash
launchctl bootout gui/$(id -u)/ai.builderpro.desktop.sessiond 2>/dev/null
rm ~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist
rm -rf ~/Library/Application\ Support/ai.builderpro.desktop
```

An in-app "remove background service" affordance is future work (see `docs/backlog.md`).

## DB quarantine (corruption recovery)

On open, a corrupt/not-a-database image is **quarantined, not fatal**: the daemon renames it to
`bpa.db.corrupt-<unix-ts>` in place and recreates a fresh database
(`crates/sessiond/src/persistence.rs`). Live sessions are unaffected (the in-memory ring is the
Layer-1 truth; persistence is best-effort). To attempt manual recovery, inspect the quarantined
file with `sqlite3`; there is no automatic re-import.

## Dev mode vs installed

- **Installed app:** launchd owns the daemon (plist above; `KeepAlive={Crashed}`).
- **Dev (`npm run tauri dev`):** SAME launchd path — there is no dev/prod branch. The core
  unconditionally installs + bootstraps + kickstarts the LaunchAgent
  (`src-tauri/src/lib.rs::ensure_daemon_running`), with the plist's daemon path resolved to the
  binary beside the current executable — i.e. **`target/debug/bpa-sessiond`**. A dev run therefore
  leaves a REAL `~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist` installed, pointing
  into your `target/debug` (a later `cargo clean` silently breaks that agent's binary path).
- **E2E harness:** the exception — it spawns `target/debug/bpa-sessiond` directly with an
  **isolated `XDG_RUNTIME_DIR`** (tempdir), so it never touches your real daemon/socket/plist
  (`tests/e2e/lib/daemon-harness.mjs`).
- **Dev cleanup** (after dev runs, not e2e): `launchctl bootout
  gui/$(id -u)/ai.builderpro.desktop.sessiond`, then remove the plist if you're done developing
  (see Uninstall) — `pkill` alone is not enough, launchd will restart a crashed daemon.

## Log rotation

**None today** — the appender is `tracing_appender::rolling::never` (single
`sessiond.tracing.log`, unbounded growth). Tracked as BL-21 in `docs/backlog.md`.

## Env-override filtering (S-EXT, closes BL-1)

`env_overrides` (the per-session env passed on `CreateSession`) is now filtered through the
shared `bpa_daemon_core::env_filter::strip_dangerous_env` helper BEFORE being merged into the
resolved child env — any `DYLD_*`/`LD_*`-prefixed key (case-sensitive) is stripped, closing the
previously-open BL-1 gap (the allowlist used to be a default, not a ceiling). This is the SAME
helper `bpa-orchd`'s stdio MCP-server spawn path uses (S-EXT, `docs/runbook-orchd.md`) — one
shared denylist, two spawn call sites, no daemon-specific copy to drift. `bpa-sessiond` itself
does not perform any outbound network I/O or Keychain access — that surface is new to `bpa-orchd`
only; see `docs/runbook-orchd.md`'s "Keychain / MCP / connector egress" section.

## Research runs (S-IDEA, `[0.7.0]`) — not this daemon

The S-IDEA research pipeline (idea→research→insight→task, `research_run` schema v4, the async run
driver + boot-reconcile) is entirely a `bpa-orchd` feature — `bpa-sessiond` has no involvement,
same as it had none in S-EXT's MCP/connector surface above. Operational notes (inspecting
`research_run` rows, what boot-reconcile does on a restart, connecting a real research server) live
in `docs/runbook-orchd.md`'s "Research runs" section, not here.
