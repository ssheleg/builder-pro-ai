# Plan — audit remediation sweep (2026-07-24)

Spec/source: [`docs/qa/2026-07-24-full-audit-report.md`](../../qa/2026-07-24-full-audit-report.md)
(every finding carries file:line, evidence, and a probe). Branch: `audit-remediation-2026-07-24`
(off `45c9836`), executed in an isolated worktree. **Definition of Done for the sweep:**
`bash scripts/final-suite.sh` → `ALL GATES PASSED`; every finding below is either **FIX** (code +
regression test in the same change, per project tenets) or **DEFER** (row added to
`docs/backlog.md` with severity + owner slice). UX-affecting fixes update `docs/ux/scenarios.md`
in the same change (CLAUDE.md hard rule) and `python3 docs/ux/lint.py` stays green.

Collision note: the main working tree is being advanced concurrently (SW1 landed as `45c9836`;
BL-108 + connector/keychain hygiene in flight on `ci.yml`, `connectors/accounts.rs`,
`secrets/lib.rs`, `sessiond/attach.rs`, `docs/backlog.md`). This branch is based on the committed
HEAD; reconciliation on merge is the owner's pass. Overlapping files are flagged per item.

## Wave A — launchd + sessiond + filesystem (Rust)

| Finding | Action | DoD |
|---|---|---|
| REL-1 (P1) | `launchd.rs::bootstrap` — "already bootstrapped" = success, **no bootout**; flip `bootstrap_already_bootstrapped_is_success` to assert no bootout; drift handling stays with the upgrade flow. | launchd unit tests green |
| REL-7 (P3) | Bound every `launchctl` invocation (small thread+channel timeout helper); write plist atomically (tmp+rename). | unit tests |
| SES-1 (P1) | Serialize create/remove per workspace: a `closing_workspaces` set gates `CreateSession` (typed `NoSuchWorkspace` while closing or when the id never existed — also fixes SES-4); after the delete tx, sweep any live session of that workspace that slipped in. | new integration test: create-storm × remove → no orphans |
| SES-4 (P2) | Covered above (unknown `workspace_id` rejected up front, not silently lost). | test |
| SES-3 (P2) | Cold-rehydrate honesty: persisted `running` maps to `exited{code:None}` (a live `atPrompt` keeps the "restored" semantics). | unit test on the rehydrate mapping |
| SES-2 (P2) | DEFER — kill-9 loses ≤1 s of scrollback by design of the 1 s flush tick; backlog row documents the window. | backlog row |
| SES-5 (P2) | OSC stripper/sanitizer: on overflow of an unterminated recognized sequence, flush the buffered bytes with the ESC byte escaped (visible inert text) instead of silently dropping user output. | unit tests both paths |
| SES-6 (P2) | `RemoveWorkspaceRoot` on the last root → typed `LastRoot` error (wire code already exists), never silent success. | integration test |
| FS-3 (P2, data-loss) | `delete/rename/move` reject any `rel` that canonicalizes to the root itself (`OutsideRoot`). | pin-tests flipped to desired |
| FS-4 (P2) | `read_file_preview` — `is_file()` guard → honest error on FIFO/special files. | pin-test flipped |
| FS-1/FS-2 (P2) | Watcher: an event on the root itself (delete/rename/overflow) yields the `["*"]` full-refresh sentinel for that root, and a vanished root yields `fs://watch-error`. | pin-tests flipped |
| FS-6 (P2) | Nested roots route by longest prefix. | pin-test flipped |
| FS-5 (P2) | Core validates `root` against registered workspace roots (cached from sessiond, invalidated on workspace events) before every fs command. If the cache plumbing grows beyond the wave, DEFER with a design row. | unit tests / backlog row |

## Wave B — orchd domain + trust + export + SW1 leftovers (Rust)

| Finding | Action | DoD |
|---|---|---|
| DOM-3 (P2) | `set_task_rank` rejects non-finite (NaN/±Inf) as `Validation`. | test |
| DOM-4 (P2) | Import: emit the family pushes even when the post-commit file write fails; the error response states "imported, file writes failed". | test |
| DOM-5 (P2) | `start_research_run` enforces `ensure_optional_project_active`. | test |
| DOM-6 (P2) | Archived guard + project-existence precheck (typed `NotFound`) for MCP server CRUD, tool upsert, skill add/delete, consent grant, policy upsert. | tests |
| DOM-7 (P2) | `mcp_server.enabled=0` enforced in connect + call_tool (typed `Policy` deny). | tests |
| DOM-8 (P2) | Import validates: no `additional` goal with NULL parent, no parent cycles, finite ranks — typed `Validation`. | tests |
| DOM-9 (P2) | Emit `GraphChanged` when insight-accept seeds the entityRef node; emit `IdeasChanged` when research-start flips lifecycle. | tests |
| DOM-10 (P2) | `ImportBundle` refuses in degraded storage (typed error); full degraded-mode gating DEFERred to backlog. | test + backlog row |
| DOM-11 (P2) | FK-violation errors map to typed `NotFound`/`Validation` instead of raw `Io`. | test |
| DOM-1/2 (P2) | Bundle gains `docs` + graph nodes/edges (additive `bundleFormat:1` keys, old keys untouched); import restores them and re-seeds entityRefs (strategic goal + accepted insights). | round-trip test |
| SEC-1 (P1) | HTTP tool calls require a current connect-consent matching the live fingerprint; any server mutation (url/command/args/env) invalidates the grant + tool cache. | tests |
| SEC-2 (P1) | stdio fingerprint = command + args + env + sha256(binary) (fallback keeps cmd-string hash). | tests |
| SEC-3 (P2) | Rate/spend check holds a per-policy async mutex across authorize+dispatch (in-flight counts). | race test: burst of 5 at cap 1 → 1 ok, 4 denied |
| SEC-4 (P2) | UI honesty: spend-cap control is labelled inert until servers report cost (strings + hint). | vitest |
| SEC-5 (P2) | `TrustGrantConsent` writes the `consent_grant` audit row. | test |
| SEC-6 (P2) | Tool-cache refresh preserves per-tool `enabled` (match by server+name). | test |
| WIP-2 (P2) | `validate_workflow` rejects zero stages (daemon-side, mirrors client). | test |
| WIP-5 (P3) | `stage.id` validated non-empty + unique. | test |

## Wave C — frontend (src/)

| Finding | Action | DoD |
|---|---|---|
| FE-1 (P2) | Generalize the stats epoch guard: one `refreshGuard` helper (per-key in-flight, dirty re-run, monotonic apply) across all `refresh*` actions; debounce `refreshInvocations`. | vitest (probe flipped) |
| FE-2 (P2) | `reportError` passes string errors through verbatim (no "unknown orchestrator error"). | vitest |
| UX-1 (P1-cand.) | Per-slice `…Fetched` flags (projects/goals/ideas/insights/tasks/servers/artifacts/researchRuns); lists render a loading row until first fetch (DocsPanel pattern). | vitest (probe flipped) |
| FE-4 (P2) | `useSubmitGuard` on: ToolsBrowser invoke, ConnectorsTab invoke, "+ New terminal", DocsPanel mutations, ProjectPanel export/import, UpgradeDialog. | vitest |
| FE-3 (P2) | `.catch(() => {})` + comment on `writeStdin`/`resize`/`orchdGraphNeighborhood` fire-and-forget paths. | vitest |
| FE-6 (P3) | Toast tone parameter (success uses `--ok` accent); call sites updated. | vitest |
| FE-7 (P3) | Restored sessions: tab marker + one-time hint when typing into a no-shell session; input not silently swallowed. | vitest |
| FS-7 (P3) | "show ignored" toggle restarts the watcher with the new flag. | vitest (probe flipped) |
| FS-8 (P3) | FileTree per-dir epoch: stale `listDir` responses are dropped after invalidation. | vitest (probe flipped) |
| UX-2 (P2) | QuickCapture + IdeasList attach selects filter to active projects. | vitest |
| GRAPH-1 (P1) | Ghost nodes: `draggable:false`/`selectable:false` and filtered out of `flushMoves`/`handleDeleteSelected` (UI-side fix now; server-side scoping DEFERred — wire change, backlog row). | vitest |
| LNK-1 (P3) | Link provider never emits a link resolved to `rel === ""` from a longer trimmed token (wrong-target kill). | vitest |
| REL-3 (P2) | ErrorBoundary "Copy details" copies the scrubbed text. | vitest |
| REL-4 (P2) | Scrubber extended: JSON-quoted keys, JWT, `gho_`/`ghu_`/`ghs_`/`ghr_`/`github_pat_`, `glpat-`, `AKIA`, `AIza`, `npm_`/`pypi-`, Slack webhooks, URL userinfo, `Cookie:`, PEM blocks, multi-word values. | vitest (corpus flipped) |
| FE-5 (P2) | Smoke-guard regex extended (`outerHTML`, `innerHTML+=`, `insertAdjacentHTML`, `document.write`, `srcDoc`); capabilities prune (`fs:default`, `fs:scope $APPDATA/**`, `store:default`); drop unused plugin-store/plugin-fs registration + JS deps + `@xterm/addon-search/serialize/web-links` (update the smoke pin accordingly). | vitest + capabilities test |
| WIP-4 (P3) | "Run workflow" picker preselects the row's workflow (by id). | vitest |

## Wave D — docs & gates (orchestrator, last)

- `docs/ux/scenarios.md` + `screens.md`: updates for every UX-affecting fix above (loading
  states, archived filters, toast tone, restored marker, LastRoot, ghosts read-only, workflow
  validated flips per WIP-3, SCN-007 nav row) → `lint.py` green.
- `final-suite.sh`: add `python3 docs/ux/lint.py` as a blocking stage (after check-english);
  `check-ux-scenarios.sh` + `CONTRIBUTING.md` repoint to `docs/ux/scenarios.md`; old catalog gets
  a superseded header (DOC-10).
- `docs/backlog.md`: new rows for every DEFER above + REL-8, ARCH-1..8 (as a tracked hardening
  set), SES-7..11, FE-9/FE-10, UX-5..9, GRP-2..4, STATS-1, SEC-7 (owner decision), BL-34
  re-evaluation note after REL-1, DOM-10 full degraded gating, FS-5 if deferred.
- Doc-lag pass: `runbook-orchd.md` (DOC-1, DOC-8), `architecture.md` 0.10.0 section + schema
  v5/v6 + survival-table link (DOC-5..7), `traceability.md` current totals/stage numbers/test
  name (DOC-3/4/12/13), CHANGELOG factual fix (DOC-9) + merged 0.10.0 headings (DOC-17) + new
  `[Unreleased]` section for this sweep, README test counts re-measured (DOC-2), `build-macos.md`
  both-daemon wording (DOC-16), SCN-058/059 status (DOC-18), BL-17 closed (DOC-14),
  BL-102 refs (DOC-15).

## Explicit non-goals

No merge to `nightbuild`/`main`, no push, no release build, no changes in the main working tree.
Server-side graph ownership wire change, generic-client extraction (ARCH-2), monolith splits
(ARCH-4), BL-34 re-design — backlog rows, not code, in this sweep.

---

## Outcome (2026-07-24)

Done: waves A (11/11), B (18/18), C (17/17), D (docs/gates), plus BL-108 (ported drain fix),
BL-102 (hermetic tests), BL-143 (server-side graph ownership, wire-compatible). `final-suite.sh`
→ ALL GATES PASSED at 11 stages (1260 Rust / 1270 TS, coverage both ≥80%, e2e ×2). Deferred: all
rows filed as BL-109..BL-156 in `docs/backlog.md`. Not in scope (as planned): merge/push/release,
GUI smoke, clean-VM runbook.
