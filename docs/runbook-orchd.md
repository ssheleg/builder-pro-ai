# Runbook — `bpa-orchd` daemon operations

Everything here uses REAL identifiers from the code (verified 2026-07-13, S3). `bpa-orchd` is the
SECOND per-user LaunchAgent — the app-domain store (projects / goals / ideas / insights / tasks /
rulesets). It has **no live runtime state to lose**: every domain row lives in `orchd.db`
(SQLite), so restarting or upgrading it never loses data the way killing `bpa-sessiond` ends live
shells. See the survival truth table in the platform overview §2 / `README.md`.

## Locations

| What | Path |
|---|---|
| LaunchAgent label | `ai.builderpro.desktop.orchd` (`src-tauri/src/launchd.rs::ORCHD_LABEL`) |
| Plist | `~/Library/LaunchAgents/ai.builderpro.desktop.orchd.plist` — **rendered at runtime** by `src-tauri/src/launchd.rs` (same mechanism as sessiond's plist, parameterized by label/binary/log names); there is no repo file to edit |
| Durable state root | `~/Library/Application Support/ai.builderpro.desktop/` (SAME dir sessiond uses — both daemons' DBs live side by side) |
| Database | `…/ai.builderpro.desktop/orchd.db` (+ `-wal`/`-shm` sidecars) |
| Tracing log | `…/ai.builderpro.desktop/logs/orchd.tracing.log` |
| launchd stdout/stderr | `…/ai.builderpro.desktop/logs/orchd.out.log` / `orchd.err.log` |
| Socket | `$XDG_RUNTIME_DIR/bpa/orchd.sock`, fallback `/tmp/bpa-<uid>/orchd.sock` (same `runtime_dir` resolution as sessiond, shared in `bpa-daemon-core::singleton`) |
| Single-instance lock | `orchd.lock` next to the socket |
| Wire version | `[1,1]` — orchd's OWN independent version space (own `bpa-orchd-proto` crate; same `BPAA` preamble magic as sessiond, daemons distinguished by socket path, not by preamble content) |
| Rules markdown files (D4's narrow file exception — NOT a general file API) | `…/ai.builderpro.desktop/rules/global.md` + `…/ai.builderpro.desktop/rules/project-<uuid>.md` (owner may repoint `md_path` elsewhere; see `docs/architecture.md`) |

## Inspect

```bash
# Is the LaunchAgent loaded / running?
launchctl print gui/$(id -u)/ai.builderpro.desktop.orchd | head -30

# Is the socket alive? (refused = daemon dead, stale file possible)
ls -la ${XDG_RUNTIME_DIR:-/tmp/bpa-$(id -u)}/bpa/orchd.sock 2>/dev/null || ls -la /tmp/bpa-$(id -u)

# Tail the daemon log
tail -f ~/Library/Application\ Support/ai.builderpro.desktop/logs/orchd.tracing.log

# Daemon process
pgrep -fl bpa-orchd
```

## Restart

> Unlike `bpa-sessiond`, restarting `bpa-orchd` does **not** end any live work — it has no PTYs,
> no live sessions. Every project/goal/idea/insight/task/ruleset row survives (SQLite), and
> in-flight requests just get a `Disconnected` the GUI retries against the reconnected daemon.

```bash
launchctl kickstart -k gui/$(id -u)/ai.builderpro.desktop.orchd
```

The GUI's `orchd_reconnect` command (`src-tauri/src/commands.rs`, the [Повторить] button on the
`orchd://down` banner) drops the client slot and re-spawns `lib.rs::bring_up_orchd`'s same
bounded-retry connect sequence used at boot — no app restart needed for a plain reconnect.

## Kickstart / bootstrap / upgrade choreography

- **Every app boot** (dev AND installed, no branch): `bring_up_orchd` mirrors `bring_up_daemon`
  EXACTLY — `install_agent()` + `bootstrap()` + `kickstart()` run UNCONDITIONALLY, then
  `connect_with_retry(client_build(), 8, 500ms)`. On `Ok` the client slot is filled and the orchd
  broker registered; on a typed `IncompatibleOrchd` the GUI shows the upgrade dialog; otherwise it
  shows the `orchd://down` banner.
- **Incompatible-daemon upgrade** (`orchd_upgrade` command, `src-tauri/src/commands.rs`): mirrors
  `upgrade_daemon` verbatim — best-effort `OrchdShutdown{drain:true}` (WAL checkpoint) →
  `kickstart_force()` (`launchctl kickstart -k`) on the orchd agent → `app.restart()` (a full app
  relaunch, same choreography as the sessiond upgrade dialog). Copy is honest that no live session
  is at risk: *«Обновить фоновый сервис оркестратора — записи (проекты, цели, задачи) сохранены»*
  (no N-live-sessions warning — orchd has no PTYs to lose).
- **Both daemons incompatible after one app update:** the GUI shows ONE dialog at a time,
  **sessiond first** (its `kickstart -k` + `app.restart()` relaunches the app; orchd's
  incompatibility re-detects on that relaunch and shows its own dialog next) — no combined
  choreography (S3 spec §10).

## Full reset (wipe orchd state)

```bash
launchctl bootout gui/$(id -u)/ai.builderpro.desktop.orchd 2>/dev/null
rm -f ~/Library/Application\ Support/ai.builderpro.desktop/orchd.db*
rm -rf ~/Library/Application\ Support/ai.builderpro.desktop/rules
rm -f "${XDG_RUNTIME_DIR:-/tmp/bpa-$(id -u)}/bpa/orchd.sock" "${XDG_RUNTIME_DIR:-/tmp/bpa-$(id -u)}/bpa/orchd.lock" 2>/dev/null
# Relaunch the app — it re-installs the plist and re-bootstraps the daemon, then re-creates
# the global ruleset row + rules/global.md idempotently at boot.
```

This wipes ONLY orchd's domain data — `bpa.db` (sessiond, terminals/workspaces) is untouched; the
two daemons' full-reset procedures are independent.

## Uninstall

```bash
launchctl bootout gui/$(id -u)/ai.builderpro.desktop.orchd 2>/dev/null
rm ~/Library/LaunchAgents/ai.builderpro.desktop.orchd.plist
# Do NOT rm -rf the shared Application Support dir here if bpa-sessiond's data must survive —
# remove orchd.db*/rules/logs/orchd.* individually (see "Full reset" above) instead.
```

## DB quarantine (corruption recovery)

On open, a corrupt/not-a-database image is **quarantined, not fatal**: the daemon renames it to
`orchd.db.corrupt-<unix-ts>` in place and recreates a fresh schema-v1 database
(`crates/orchd/src/persistence.rs`, mirrors `bpa-sessiond`'s identical quarantine behavior). To
attempt manual recovery, inspect the quarantined file with `sqlite3`; there is no automatic
re-import — use `ExportAll`/`ImportBundle` (a project or whole-store JSON export/import, see the
S3 spec §8) as the supported recovery path once the daemon is back up on a fresh DB.

## Dev mode vs installed

- **Installed app:** launchd owns the daemon (plist above; same `KeepAlive={Crashed}` semantics as
  sessiond — both are the SAME `LaunchdAgent` struct type from `src-tauri/src/launchd.rs`,
  instantiated with the orchd identity in `lib.rs::build_orchd_launchd_agent` — sessiond's own
  construction site stays byte-identical, proven by test).
- **Dev (`npm run tauri dev`):** SAME launchd path — `bring_up_orchd` unconditionally installs +
  bootstraps + kickstarts, with the plist's daemon path resolved to
  `LaunchdAgent::resolve_daemon_path("bpa-orchd")` — i.e. **`target/debug/bpa-orchd`**. A dev run
  therefore leaves a REAL `~/Library/LaunchAgents/ai.builderpro.desktop.orchd.plist` installed,
  pointing into your `target/debug` (a later `cargo clean` silently breaks that agent's binary
  path, exactly like sessiond).
- **E2E harness:** the exception — `tests/e2e/orchd-survive.mjs` spawns `target/debug/bpa-orchd`
  directly with an **isolated `XDG_RUNTIME_DIR`/`HOME`** (tempdirs), so it never touches your real
  daemon/socket/plist/DB (reuses `tests/e2e/lib/daemon-harness.mjs`'s `spawnDaemon`/framing code,
  protocol-agnostic).
- **Dev cleanup** (after dev runs, not e2e): `launchctl bootout
  gui/$(id -u)/ai.builderpro.desktop.orchd`, then remove the plist if you're done developing (see
  Uninstall) — `pkill` alone is not enough, launchd restarts a crashed daemon.

## Log rotation

**None today** — same as sessiond, the appender is `tracing_appender::rolling::never` (single
`orchd.tracing.log`, unbounded growth, shared `bpa_daemon_core::logging::init_tracing`). Tracked
as BL-21 in `docs/backlog.md` (now applies to both daemons).

## Related docs

- `docs/runbook-daemon.md` — the `bpa-sessiond` runbook this one mirrors.
- `docs/architecture.md` — two-daemon topology, module map, the D4 rules-md file exception.
- `docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md` — the locked S3 spec
  this daemon is derived from (§2.1 names table, §5 boot/schema, §9 core integration).
