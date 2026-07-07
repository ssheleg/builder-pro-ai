# Backlog — accepted-deferred findings (canonical)

Rule (normative, see CONTRIBUTING.md): any accepted-deferred finding MUST land here
in the same change that defers it. Gitignored ledgers are working notes, never the record.

Severity: P1 security/correctness · P2 robustness/ops · P3 polish.
Status: open · in-progress · done (row stays, link the closing commit).

| ID | Severity | Area | Summary | Origin | Owner slice | Status |
|----|----------|------|---------|--------|-------------|--------|
| BL-1 | P1 | daemon/security | Enforce `DYLD_*`/`LD_*` denylist on `env_overrides` before applying (allowlist is a default, not a ceiling, until then) | Audit A11; spec §16 decision | Pre-S6 hardening | open |
| BL-2 | P1 | webview/security | Set restrictive CSP in `tauri.conf.json` (currently `csp: null`); LLM egress core-only rule | Audit A19 | Pre-S6 hardening | open |
| BL-3 | P1 | daemon/security | `bpa.db` file mode 0600 + purge-on-delete (scrollback carries secrets) | Audit A18 | Pre-S6 hardening | open |
| BL-4 | P2 | daemon | Session/workspace deletion + retention (20 exited/workspace, 30-day TTL, cascade with consent) + `SessionLimitReached` cap | Audit A15, A21; spec P3 | Protocol v2 / S2 | open |
| BL-5 | P2 | frontend | Reconnect stale-pane UX: mark stale until re-attached; dim/disable panes while disconnected | Audit A22 | S2 UI pass | open |
| BL-6 | P2 | frontend | Error-surfacing implementation: code→message table, toasts, catch on all fire-and-forget invokes (incl. `void manager.attach`) | Audit A23 | S2 UI pass | open |
| BL-7 | P2 | e2e/daemon | Daemon-restart rehydration e2e phase (SIGTERM daemon, relaunch, assert inactive rehydrate + scrollback) + persistence-degraded Push event | Audit A12 | Protocol v2 | done (Task 12/12r — e2e phase 5, `tests/e2e/survive-restart.mjs`, green; persistence-degraded Push event remains a separate, still-open follow-on if ever needed) |
| BL-8 | P2 | daemon | Scrollback flush dirty-check (skip unchanged rings) + DB write-architecture review | Audit A24 | Protocol v2 / S3 | open |
| BL-9 | P3 | daemon | `flush_scrollback_once`: batch per-tick DB-lock acquisition (head-of-line latency) | Final review deferral | S3 | open |
| BL-10 | P3 | daemon | `pty_supervisor` sink MutexGuard held across `send()` — hardening vs future bounded channel | Final review deferral | any | open |
| BL-11 | P2 | daemon | Escaped-descendant stream: keep cancel reachable after `remove_session`; Output-after-ChildExited when a descendant holds the PTY slave | Truncation-fix verification | Protocol v2 | open (re-evaluated under Pv2 multi-sink fan-out — `crates/sessiond/src/pty_supervisor.rs` `Shared.sinks: Mutex<Vec<(sub_id, Sink)>>`: the reasoning is unchanged and now applies PER-SINK — `remove_session` still lets each subscriber's forwarder drain gracefully rather than cancel, and a descendant holding the PTY slave open after exit can still emit Output-after-ChildExited to any one subscriber independently of the others; no fix, still deferred) |
| BL-12 | P3 | daemon | Stale attach entry when child exits during in-flight attach Replay send (count over-reports until conn close) | Truncation-fix verification | any | open |
| BL-13 | P3 | core/tests | Preflight call-site wiring untested (`preflight_cwd`/`preflight_workspace_root` calls in command fns need a State-driven test) | Truncation-fix verification | any | open |
| BL-14 | P2 | frontend | `applyReplay` without `term.reset()` → duplicated scrollback on any re-attach | A1 verification; chip task_ada4835d | S2 UI pass | open |
| BL-15 | P3 | core | Cross-layer stale-channel windows: 30 s-hang double-replay; unconditional `remove_attachment` on failed attach | Attach-dedup verification | Protocol v2 | open |
| BL-16 | P2 | test-infra | `singleton.rs` env-mutation race (needs process-wide env lock); once-seen attach parallel flake; real-shell integration tests silently no-op when the shell binary is absent — CI must assert they ran (ci.yml does since this cycle) | Ledger CI-TODO | CI hardening | open |
| BL-17 | P2 | CI | Coverage gate ≥80 % sessiond enforced in CI (job added this cycle — verify it stays blocking); local run optional | Spec §14.3; audit A20 | this cycle → done when CI green | open |
| BL-18 | P3 | bundle | Prune ~28 unused iOS/Android icon assets from `src-tauri/icons/` | Final review deferral | any | open |
| BL-19 | P3 | release | Tauri auto-updater channel (manifests, hosting) — manual DMG until then | Owner decision D4 | post-S2 | open |
| BL-20 | P1 | agents/security | macOS Keychain storage for provider API keys (never SQLite/config/logs) | Audit A9; spec §16 | S6a | open |
| BL-21 | P2 | daemon/ops | Log rotation: appender is `rolling::never` (single unbounded `sessiond.tracing.log`) — switch to daily or size-capped | Runbook truth pass (this cycle) | any | open |
| BL-22 | P1 | mcp/security | MCP hardening: tool-result prompt-injection mediation, per-server connect consent, stdio-server execution consent, spend caps via the trust layer | Vision-v2 audit V13 | S-EXT | open |
| BL-23 | P3 | ux/ops | Remote escalation channel — answer a hot question away from the Mac (macOS notification only in v0.x; Telegram/mobile web later) | Owner decision Q3 | post-S6c | open |
| BL-24 | P2 | data/security | Ingested-telemetry retention + redaction: prod logs/errors + metrics carry PII/secrets — store home, redaction, retention, purge-on-project-delete | Charter telemetry class; Q15 | S9a | open |
| BL-25 | P3 | product | First-class Experiment entity (hypothesis / budget / metric / verdict); v1 uses Tasks tagged `experiment` | Owner decision Q11 | S8 | open |
| BL-26 | P2 | orchd | StepRun payload-thinning job: full I/O payloads 14 days → thinned to metadata (summaries + sizes + hashes); 50 MB/run cap with honest truncation markers | Owner decision Q15 | SW1 | open |
| BL-27 | P2 | orchd/security | Keychain access for `bpa-orchd` unattended runs while the screen is LOCKED — resolve before the first unattended MCP/provider call | ADR-HOST sub-question | S-EXT | open |
| BL-28 | P3 | tests | Shared test-support MockLaunchctl: `commands.rs` tests structurally duplicate `launchd::tests::MockLaunchctl` (private) — extract a shared test-support module | Task 10a review | any | open |
| BL-29 | P3 | ux/a11y | Explicit `:focus-visible` CSS (2px accent ring) app-wide — design-system promises it, components rely on UA default; also `within()`-scope duplicate «Обновить» accessible names (banner + dialog co-mounted) in future full-app tests | Task 10b review | S2 | open |
| BL-30 | P2 | daemon/perf | Cold-rehydrate loads ALL persisted sessions' scrollback into RAM at boot (≤256 KiB × N sessions) — fine at v0.x scale; consider lazy per-session load before workspaces with many dead sessions | Task 12r | S3 | open |
| BL-31 | P3 | daemon | `command_events` not reconciled on cold-rehydrate (rehydrated sessions carry no in-memory event queue; persisted rows remain readable) — reconcile when a consumer appears | Task 12r review | SW1 | open |
| BL-32 | P3 | daemon | DaemonShutdown Ack races process exit at PROCESS scope: serve()/run() never join per-connection tasks, so on a zero-session fast drain the enqueued Ack (and a second client's in-flight reply) can be dropped before the writer flushes; e2e passes because real drains are slower — add a bounded quiesce/join of client tasks before exit | Pv2 final review (Minor #6) | Pv2.1 | open |
| BL-33 | P3 | daemon | Replay-only attach during the reap-vs-reader-drain window can deliver a permanently truncated Replay (ChildExited broadcast before the reader finishes draining the PTY; ReplayOnly entry never receives the tail) — tight race, bounded loss; align reap ordering or delay inactive-attach until reader-drain completes | Pv2 final review (Minor #8) | Pv2.1 | open |
