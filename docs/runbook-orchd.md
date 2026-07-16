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

The GUI's `orchd_reconnect` command (`src-tauri/src/commands.rs`, the [Retry] button on the
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
  is at risk: *«Update the orchestrator background service — your records (projects, goals, tasks) are preserved»*
  (no N-live-sessions warning — orchd has no PTYs to lose).
- **Both daemons incompatible after one app update:** the GUI shows ONE dialog at a time,
  **sessiond first** (its `kickstart -k` + `app.restart()` relaunches the app; orchd's
  incompatibility re-detects on that relaunch and shows its own dialog next) — no combined
  choreography (S3 spec §10).

## Resetting the local database

The one-time developer reset: drop just the SQLite store (and its `-wal`/`-shm` sidecars) while
leaving the LaunchAgent, rules markdown, logs, and socket in place. The daemon recreates a fresh
schema-v1 `orchd.db` at its next boot, so this is the fastest way to start from an empty domain
store on your own machine (e.g. after a dev run left stale rows).

```bash
launchctl bootout gui/$(id -u)/ai.builderpro.desktop.orchd 2>/dev/null
rm -f ~/Library/Application\ Support/ai.builderpro.desktop/orchd.db*
# Relaunch the app (or `launchctl kickstart -k gui/$(id -u)/ai.builderpro.desktop.orchd`) —
# boot recreates orchd.db with schema v1 and re-seeds the global ruleset row idempotently.
```

The `orchd.db*` glob covers `orchd.db`, `orchd.db-wal`, and `orchd.db-shm`. This touches ONLY
orchd's domain data — `bpa.db` (sessiond, terminals/workspaces) is untouched. To also clear the
rules markdown and socket/lock, use "Full reset" below instead.

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

## Storage-degradation modes (S-POLISH, BL-94)

The daemon decides its **storage mode ONCE at boot** — in `boot::open_db_degrading`
(`crates/orchd/src/boot.rs`, over `Db::open_with_outcome` in `persistence.rs`) — and never changes
it again without a restart. There is no background re-check and no push; the mode is a boot fact. It
is carried in `ServerDeps.storage_status` and returned verbatim by the `GetStorageStatus` dispatch
arm; the GUI pulls it via the `orchd_storage_status` command on connect and on every reconnect. The
three modes (`StorageMode` in `crates/orchd-proto/src/lib.rs`, snake_case on the wire):

| Mode | What happened at boot | Durability | Banner? |
|---|---|---|---|
| `persistent` | `orchd.db` opened cleanly | **Full** — every row is on-disk SQLite as usual | No |
| `recovered_from_corruption` | A corrupt / not-a-database image was quarantined aside and a fresh schema DB recreated | New writes are durable again; the **pre-corruption data survives only in the quarantined file** | Yes (with the path) |
| `in_memory_fallback` | The disk was unusable (`create_dir_all`/on-disk open both failed), so the daemon fell back to a non-persistent in-memory DB | **None across restart** — everything works this session, nothing survives a daemon stop | Yes |

**The frontend surfaces an honest banner for the two non-persistent modes**
(`recovered_from_corruption` and `in_memory_fallback`) and nothing for `persistent` — so the owner
is never silently told their data is durable when it isn't. `recovered_from_corruption` carries a
`quarantinedPath`; `in_memory_fallback` carries none (`quarantined_path` is `None`).

**Where the quarantined corrupt DB lands:** `…/ai.builderpro.desktop/orchd.db.corrupt-<unix-ts>`
(same Application Support dir as `orchd.db`, seconds-resolution suffix — see the "DB quarantine"
section above; the rename is done in place by `persistence.rs`). It is **left on disk untouched**
for manual recovery — inspect it with `sqlite3`, then re-import via `ExportAll`/`ImportBundle`.

**Confirm the current mode operationally:**

```bash
# recovered_from_corruption leaves a quarantine file behind:
ls -la ~/Library/Application\ Support/ai.builderpro.desktop/orchd.db.corrupt-* 2>/dev/null

# in_memory_fallback logs a degradation line at boot (persistent/recovered do not):
grep -i "degraded (in-memory) mode" \
  ~/Library/Application\ Support/ai.builderpro.desktop/logs/orchd.tracing.log
```

An `in_memory_fallback` almost always means the Application Support dir is unwritable (permissions,
a full or read-only disk) — fix the underlying disk/permission problem and restart the daemon; the
next boot re-opens `orchd.db` on-disk and returns to `persistent`.

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

## Per-request tracing fields (S-POLISH, O-6)

Every dispatched request emits exactly ONE structured completion line, from a single wrapper around
the dispatch loop (`crates/orchd/src/socket_server.rs`) — no per-verb log edits. The line carries
only this low-cardinality quartet:

| Field | Meaning |
|---|---|
| `verb` | The request variant name (`OrchdRequest::verb_name`, an exhaustive compile-time-checked match — a new verb fails to build until it is named) |
| `outcome` | `"ok"` or `"err"` (derived from whether the response is `OrchdResponse::Error`) |
| `error_code` | The `OrchdErrorCode` name — present **only** on an error line |
| `elapsed_ms` | Wall-clock dispatch time in milliseconds |

The message text is `request completed`. Grep a session's request timeline (and spot slow or
failing verbs) with:

```bash
grep "request completed" ~/Library/Application\ Support/ai.builderpro.desktop/logs/orchd.tracing.log
```

**No secrets, args, bodies, tokens, tool output, ids, or PII are ever in this line** — only the
quartet above, enforced by `crates/orchd/tests/no_secrets_in_logs_tracing.rs`. The same single-line
convention is applied at the two other choke-points: the Tauri core's `orchd_client::request`
(`src-tauri/src/orchd_client.rs`, message `orchd request completed`) and `bpa-sessiond`'s own
dispatch wrapper (`crates/sessiond/src/socket_server.rs`, see `docs/runbook-daemon.md`) — so a
request can be followed end-to-end (core → daemon) by the same `verb`/`outcome`/`error_code`/
`elapsed_ms` fields on both sides.

## Keychain / MCP / connector egress (S-EXT, `[0.6.0]`)

`bpa-orchd` is the app's first process to perform outbound network I/O and macOS Keychain access
(never `bpa-sessiond`, never the GUI core — see `docs/architecture.md`'s "Extensions" section for
the full design). Nothing here is a new daemon or a new socket — it's new *behavior* inside the
existing orchd process, gated by the trust choke-point (`crates/orchd/src/trust.rs`) on every
connect/spawn/call.

**Keychain entries this daemon creates:**

| What | Keychain service | Keychain account |
|---|---|---|
| An MCP server's bearer token (`McpSetServerBearer`) | `ai.builderpro.desktop.mcp` | the server's uuid |
| A connector account's OAuth access token | `ai.builderpro.desktop.account` | `<account-uuid>:token` |
| A connector account's OAuth refresh token | `ai.builderpro.desktop.account` | `<account-uuid>:refresh` |
| A connector account's static api-key | `ai.builderpro.desktop.account` | `<account-uuid>:apikey` |

`orchd.db` never stores the secret bytes — only the Keychain account-key string above (the
`secret_ref`/`refresh_ref` columns on `mcp_server`/`account`). Deleting a server (`McpDeleteServer`)
or an account (`ConnectorDeleteAccount`) deletes the matching Keychain entry(ies) too — inspect
with `security find-generic-password -s ai.builderpro.desktop.mcp -a <server-id>` (or the
`.account` service for a connector) if you need to confirm one exists/was removed; never `-w` to
print the secret value in a shared terminal/log.

**What can reach the network:** ONLY an MCP server the owner explicitly registered + granted
connect (or `stdio_exec`) consent to, and ONLY a connector account/adapter the owner explicitly
added — there is no background polling, no telemetry call, no unsolicited egress. Every attempt
(allow or deny) writes an `audit_log` row (`TrustListAudit`, surfaced in the «Extensions» →
Log tab) — `reason` never contains a secret or a tool argument, only a short code like
`consent_required`/`spend_cap_exceeded`.

**stdio MCP servers spawn a real child process:** `pgrep -fl` under `bpa-orchd`'s pid will show
it like any other child; it inherits NO ambient env (the `DYLD_*`/`LD_*` denylist + `env_clear()`
discipline in `crates/mcp/src/transport.rs`/`crates/orchd/src/mcp/mod.rs` — the same shared
`env_filter` helper `bpa-sessiond` now applies to `env_overrides`, `docs/runbook-daemon.md`). Its
stderr is currently inherited straight to orchd's own log stream, unredacted (BL-69) — be aware
before running an untrusted/third-party stdio server, and don't paste `orchd.tracing.log` output
publicly without checking it first if any stdio server has been connected.

**Human step — connecting a real provider (e.g. prowl.chat):** the autonomous/CI path proves the
whole MCP + connector mechanism against a LOCAL STUB server (`tests/e2e/orchd-survive.mjs` phases
6/7) — no agent creates or enters a real credential. To connect a REAL server: open «Extensions» →
Servers → add a server (name + URL) → «Connect» (grants connect consent) → if it needs a
bearer, «set token» (masked, never echoed back) and paste it yourself. For a real OAuth
connector: «Extensions» → Connectors → «connect OAuth» — this needs an owner-registered OAuth
client (the v1 provider registry ships EMPTY; `register_oauth_provider` must be called with a
real IdP's client id/secret/endpoints before `ConnectorBeginOAuth` can succeed for that provider —
there is no config-file-backed registry yet, D14 Phase 3 follow-up) — or «add API key» for a
simpler static key. None of this is on the automated test path; it's a one-time, owner-performed
setup step.

## Research runs (S-IDEA, `[0.7.0]`)

Research is a domain feature INSIDE `bpa-orchd` — `ResearchStartRun` calls the identical
`mcp::invoke::call_tool` path the section above documents, so everything about connect consent,
Keychain-held bearers, and egress boundaries above applies unchanged. This section covers the
research-specific operational surface: the `research_run` table and the async run driver.

**Where the data lives:** `research_run` rows are ordinary rows in `orchd.db` (schema v4,
`SCHEMA_VERSION` 3→4) — no separate store. Inspect them directly:

```bash
sqlite3 ~/Library/Application\ Support/ai.builderpro.desktop/orchd.db \
  "SELECT id, idea_id, status, error_kind, artifact_id, created_at FROM research_run ORDER BY created_at DESC LIMIT 20;"
```

A `done` row's `artifact_id` points at an `mcp_artifact` row (the SAME table `McpCallTool`/
`ConnectorInvoke` write to — there is no separate research-artifact table, S-IDEA spec D2); join
against `mcp_artifact` to see the actual research content.

**The async run driver is orchd's FIRST long-lived background task** — `ResearchStartRun` spawns a
detached `tokio::spawn`, distinct from every other request handler (which complete and return
within one dispatch call). This has one operational consequence worth knowing: **a `research_run`
can be `running` at the exact moment the daemon stops** (restart, upgrade, or crash) — unlike every
other row in `orchd.db`, which is always in a fully-committed, terminal state between requests.
`kickstart -k` (the Restart section above) or any daemon stop while a run is `running` interrupts
it; **the boot-reconcile step handles this automatically, no manual intervention needed**: right
after `open_db` on every boot, `Db::reconcile_interrupted_research_runs` flips any row still
`pending`/`running` to `failed{interrupted}` (D11) — the owner just re-runs it from «Research».
You can confirm this happened by tailing the boot log around a restart:

```bash
tail -f ~/Library/Application\ Support/ai.builderpro.desktop/logs/orchd.tracing.log | grep -i reconcile
```

There is no owner action required here — this is the daemon healing itself, documented so a
`failed{interrupted}` row in the query above isn't mistaken for a code bug when you see one after a
restart during an in-progress research run.

**Connecting a real research server** is the SAME "Human step" as above — `ResearchRunDialog`'s
server picker only lists servers already registered in «Extensions» → Servers, so connecting
prowl.chat (or any other MCP research tool) as a real server is a one-time prerequisite before
`ResearchStartRun` can target it; the autonomous/e2e path (phases 8/9, `tests/e2e/orchd-survive.mjs`)
proves the identical mechanism against a local stub, never a real credential.

## Related docs

- `docs/runbook-daemon.md` — the `bpa-sessiond` runbook this one mirrors.
- `docs/architecture.md` — two-daemon topology, module map, the D4 rules-md file exception.
- `docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md` — the locked S3 spec
  this daemon is derived from (§2.1 names table, §5 boot/schema, §9 core integration).
- `docs/superpowers/specs/2026-07-15-s-idea-research-pipeline-design.md` — the locked S-IDEA spec
  the "Research runs" section above summarizes (§4 schema, §6 the run driver, D11 boot-reconcile).
