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
| BL-7 | P2 | e2e/daemon | Daemon-restart rehydration e2e phase (SIGTERM daemon, relaunch, assert inactive rehydrate + scrollback) + persistence-degraded Push event | Audit A12 | Protocol v2 | open |
| BL-8 | P2 | daemon | Scrollback flush dirty-check (skip unchanged rings) + DB write-architecture review | Audit A24 | Protocol v2 / S3 | open |
| BL-9 | P3 | daemon | `flush_scrollback_once`: batch per-tick DB-lock acquisition (head-of-line latency) | Final review deferral | S3 | open |
| BL-10 | P3 | daemon | `pty_supervisor` sink MutexGuard held across `send()` — hardening vs future bounded channel | Final review deferral | any | open |
| BL-11 | P2 | daemon | Escaped-descendant stream: keep cancel reachable after `remove_session`; Output-after-ChildExited when a descendant holds the PTY slave | Truncation-fix verification | Protocol v2 | open |
| BL-12 | P3 | daemon | Stale attach entry when child exits during in-flight attach Replay send (count over-reports until conn close) | Truncation-fix verification | any | open |
| BL-13 | P3 | core/tests | Preflight call-site wiring untested (`preflight_cwd`/`preflight_workspace_root` calls in command fns need a State-driven test) | Truncation-fix verification | any | open |
| BL-14 | P2 | frontend | `applyReplay` without `term.reset()` → duplicated scrollback on any re-attach | A1 verification; chip task_ada4835d | S2 UI pass | open |
| BL-15 | P3 | core | Cross-layer stale-channel windows: 30 s-hang double-replay; unconditional `remove_attachment` on failed attach | Attach-dedup verification | Protocol v2 | open |
| BL-16 | P2 | test-infra | `singleton.rs` env-mutation race (needs process-wide env lock); once-seen attach parallel flake | Ledger CI-TODO | CI hardening | open |
| BL-17 | P2 | CI | Coverage gate ≥80 % sessiond enforced in CI (job added this cycle — verify it stays blocking); local run optional | Spec §14.3; audit A20 | this cycle → done when CI green | open |
| BL-18 | P3 | bundle | Prune ~28 unused iOS/Android icon assets from `src-tauri/icons/` | Final review deferral | any | open |
| BL-19 | P3 | release | Tauri auto-updater channel (manifests, hosting) — manual DMG until then | Owner decision D4 | post-S2 | open |
| BL-20 | P1 | agents/security | macOS Keychain storage for provider API keys (never SQLite/config/logs) | Audit A9; spec §16 | S6a | open |
