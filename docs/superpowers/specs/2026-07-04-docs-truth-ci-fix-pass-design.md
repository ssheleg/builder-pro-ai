# Builder Pro AI — Docs Truth + CI Fix Pass (Cycle 1) — Design

**Date:** 2026-07-04
**Status:** approved design (brainstormed with owner; all product decisions recorded below)
**Inputs:**
- Whole-docs audit: [`docs/superpowers/research/2026-07-03-docs-spec-audit.json`](../research/2026-07-03-docs-spec-audit.json)
  (32 verified findings A1–A32; A1 already fixed in code on `main` @ `68d3e89`/`4c91dfe`/`285cb2e`)
- Original product vision: [`docs/superpowers/research/2026-07-01-product-vision.md`](../research/2026-07-01-product-vision.md)
- S0+S1 slice: MERGED to `main` @ `285cb2e`; all gates green (205 Rust / 107 TS / clippy `-D warnings` /
  ts-rs parity / e2e survive-restart).

## 0. Owner decisions (locked — do not re-litigate in the plan)

| # | Decision | Choice |
|---|----------|--------|
| D1 | Scope of this cycle | **Docs + CI only.** No product-code changes. Codec migration is Cycle 2. Remaining code follow-ups → `docs/backlog.md`. |
| D2 | Canonical backlog home | **`docs/backlog.md` in-repo.** Process rule: accepted-deferred findings land there, never only in gitignored ledgers. |
| D3 | Wire-codec trajectory | **Migrate** to a tagged-enum-safe codec (postcard or bincode 2 serde-compat — Cycle 2 spec decides after Context7 doc check), bundled with version negotiation as **protocol v2**, pre-S2. This cycle only *documents* the current bridge truthfully and writes the evolution policy. |
| D4 | Daemon upgrade UX (v0.x) | **Consent dialog** («Update background service — N live sessions will end; records+scrollback survive») + real drain via SIGTERM + `launchctl kickstart -k`. Releases = **manual notarized DMG**; Tauri auto-updater = roadmap line only. |
| D5 | Human+agent co-viewing (S6) | **Wire-level multi-subscriber attach** is the plan of record (N subscribers per session, each with its own Replay). Protocol shape is designed in Cycle 2 (same wire break as codec v2); this cycle records the contract in overview §4. |
| D6 | CEO ⇄ knowledge graph | **S4 (knowledge graph) is a HARD prerequisite of S6 (agents).** Roadmap reorders accordingly. |
| P1 | Agent brake posture (§16 roadmap) | Approval-gate classes: destructive-fs outside workspace, `git push --force`, package publish, spend/payment, prod deploy. Batched via escalation inbox; synchronous confirm only for irreversibly destructive actions. Implementation: S6c. |
| P2 | SSH_AUTH_SOCK | **Withheld by default for agent-spawned sessions**, grantable per-task via approval. Human GUI sessions keep current behavior. |
| P3 | Retention defaults | Keep last **20 exited sessions per workspace** / **30-day TTL** (config later); workspace delete cascades live sessions **with consent**. Implementation: backlog. |
| P4 | GitHub | Create **private GitHub repo now** (`gh` авторизован, repo scope), push `main`, CI verified live in this cycle — not a human step. |

## 1. Goals / non-goals

**Goals.**
1. Every tracked doc tells the truth about the built system (kill A2/A12/A24/A30 doc-rot).
2. Forward contracts for S2–S8 exist at roadmap level: protocol evolution, agent platform,
   security posture, data charter, retention/resource model (kill A3–A9, A11, A13–A19, A21–A23
   at the *documentation/contract* level).
3. The deferred backlog is durable and process-protected (kill A10 + recurrence).
4. Quality gates are machine-enforced: CI on GitHub Actions runs everything `final-suite.sh`
   runs plus `fmt --check` and `tsc --noEmit`; toolchain pinned (kill A20, A31).

**Non-goals (explicitly out).**
- No product code changes. The only non-doc files touched: CI/workflow config, `rust-toolchain.toml`,
  `package.json` `engines` field, `CHANGELOG.md`, `.gitignore` if needed.
- Codec migration, version negotiation, multi-subscriber attach implementation → **Cycle 2**.
- Code follow-ups (reconnect UX, error-toast surface, deletion/retention impl, DYLD denylist,
  CSP hardening, db 0600, persistence-degraded event, daemon-restart e2e, replay-reset) →
  transcribed into `docs/backlog.md` with owner-slice tags.

## 2. Deliverables (file-ownership map — one owner task per file, parallel-safe)

| # | File | Action |
|---|------|--------|
| F1 | `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` | Amend in place + changelog block at top |
| F2 | `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` | Rewrite §2 table wording, §4; add new sections; re-decompose roadmap |
| F3 | `docs/backlog.md` | NEW — canonical backlog (schema + seed items in §5) |
| F4 | `docs/runbook-daemon.md` | NEW — daemon operations runbook |
| F5 | `CONTRIBUTING.md` | NEW — process rules incl. D2 backlog rule, TDD/DoD, planning cycle |
| F6 | `docs/frontend-conventions.md` | NEW — frontend platform contract |
| F7 | `.github/workflows/ci.yml` | NEW — macOS CI workflow |
| F8 | `rust-toolchain.toml` | NEW — pin stable 1.92 |
| F9 | `CHANGELOG.md` | NEW — seeded with S0+S1 entry |
| F10 | `package.json` | Add `engines` (Node ≥ current major used); no dep changes |
| F11 | `README.md` | Truth pass: survival wording, quickstart fix (sessiond build step), gates list, CI badge |
| F12 | `docs/architecture.md` | Truth pass: dual-codec note + module map (add `bpa-paths`), remove "no protocol drift is possible" overclaim |
| F13 | `docs/traceability.md` | Rows updated where claims change (esp. e2e survive scope; A12) |
| F14 | `docs/build-macos.md` | `bpa-paths` in workspace list; release-security-posture statement; smoke-VM prereqs fix |
| F15 | `scripts/final-suite.sh` | Add `cargo fmt --check` + `npx tsc --noEmit` stages (docs claim parity with CI) |
| — | GitHub | `gh repo create` (private) + push `main` + fix-pass branch; CI green live |

## 3. Locked content per deliverable

### 3.1 F1 — S0+S1 spec amendments (in place; each edit cites its finding)

Add at top: `## Post-implementation amendments (2026-07-04)` — changelog table (edit → finding → date).

1. **§3 + §5 + §6.2 codec truth (A2):** replace "bincode is serde-native for these enums" claims with:
   bincode 1.3.3 cannot *deserialize* internally/adjacently-tagged serde enums; `SessionLifecycle`
   and `TerminalEvent` ship hand-written `Serialize`/`Deserialize` impls that branch on
   `is_human_readable()` and tunnel a JSON string inside the bincode frame (dual codec). Locked
   contract box: **«DO NOT re-derive Serialize/Deserialize on these types; DO NOT add new
   serde-tagged enums to the Hop-B protocol»** — until protocol v2 (Cycle 2) replaces the codec
   (reference D3). Cross-link `crates/protocol/src/lib.rs` dual-codec comment and
   `crates/protocol/tests/roundtrip.rs`.
2. **§15 «verify at lock» resolution table (A2/A30):** each §15 item gets its resolution: what was
   verified, what diverged (15.5 codec — diverged, see §3 amendment; 15.3 binary Channel payload —
   record actual decision as built).
3. **§11 persistence truth (A24):** flush cadence = `SCROLLBACK_FLUSH_INTERVAL` 1000 ms (not
   «~500 ms»); delete the phantom 32 KiB size-trigger claim (never built); document seq-0
   snapshot-replace semantics + crash window (best-effort, bounded by interval); lock the 256 KiB
   replay cap as a named constant of the contract; add note: dirty-check + write-architecture
   review deferred (backlog, A24-impl).
4. **§13 + §12 attach/reconnect/error contracts (A22/A23):**
   - Attach-once invariant is **per session** (registry in `TerminalManager`, generation-guarded;
     as built @ `4c91dfe`). Document coalescing + reset semantics.
   - Reconnect contract: on `daemon://reconnected` → reset ALL attach state; eager re-attach
     visible session; hidden sessions re-attach lazily on next show with fresh Replay. Stale panes
     must be visually marked until re-attached (impl: backlog).
   - Error-surfacing contract: table `CommandError`/daemon error code → user-visible message;
     invariant «no command rejection is silent»; implementation = backlog item (A23-impl), the
     *contract* is normative now.
5. **§16 rewrite (A7/A9/A11/A18/A19 + P1/P2):** consolidated trust model:
   - **Agent execution trust layer** (roadmap, implemented S6c): P1 approval classes verbatim;
     append-only audit log of agent-initiated commands; per-client identity decision recorded
     (same-uid peer-cred today; agent identity tokens = S6 design).
   - **SSH_AUTH_SOCK:** P2 verbatim.
   - **env_overrides:** decision — daemon WILL enforce a `DYLD_*`/`LD_*` denylist (code: backlog
     A11-impl); until then the §9.3 allowlist is a default, not a ceiling — stated honestly.
   - **Provider API keys:** macOS Keychain only; never in SQLite/config/logs; LLM egress from the
     core process only (webview never talks to providers). (S6a implements.)
   - **Webview boundary:** restrictive CSP to be set in `tauri.conf.json` (backlog A19-impl);
     document current `csp: null` honestly until then.
   - **Data at rest:** `bpa.db` contains raw terminal scrollback ⇒ treat as secret-bearing; 0600
     (backlog A18-impl), purge-on-delete tie-in (P3), retention defaults P3 verbatim.
6. **Resource model statement (A21), §9/§13 amendment:** document today's honest posture — no
   enforced limit on concurrent sessions; each live session costs 3 OS threads (+1 forwarder per
   attachment) and open fds; exhaustion behavior is whatever the OS does. Define the contract-level
   remedy as a planned additive change: `SessionLimitReached` typed error + configurable cap
   (implementation: backlog item 4). Add the per-session cost envelope to `docs/architecture.md`
   (F12 carries the same numbers).
7. **Survival truth (A12), §13 + §2-references:** wording everywhere becomes: «**GUI/app restart:**
   live shells KEEP RUNNING (daemon-owned). **Daemon restart/upgrade/crash:** live shells END;
   session records + scrollback survive and rehydrate as inactive sessions.» Fix the traceability
   claim: e2e proves *client*-restart survival (phase 4), daemon-restart rehydration e2e = backlog
   (A12-impl). Add persistence-degraded Push event to the §13 degradation matrix as a specced
   future event (backlog).

### 3.2 F2 — Overview changes

1. **§2 survival truth table:** re-worded per §3.1(6); add row «agent runs» → *undefined until S6b
   (agent state is NOT covered by the daemon survival model)* — honest placeholder.
2. **NEW «Protocol evolution & upgrade policy» section (A3/A14/D3/D4):**
   - Append-only wire discipline (variant order frozen; additive fields only) + cross-version
     decode tests as a standing requirement.
   - Version negotiation: client sends supported range; daemon answers in-range or
     `Incompatible{min,max}` with remediation UX (dialog per D4). Implemented in Cycle 2.
   - Daemon upgrade choreography (D4 verbatim); `DaemonShutdown{drain}` is currently a no-op Ack —
     marked **reserved**; real drain semantics defined in Cycle 2.
   - Release channel: manual notarized DMG now; Tauri auto-updater = named roadmap row.
   - Workspace evolution note (A14): multi-root workspaces arrive as additive
     `workspace_roots: Vec<PathBuf>` alongside `root_path` (compat), slice S2.
3. **§4 rewrite — agent-platform forward contract (A4/A5/A6/A16/D5):**
   - Hop-B socket protocol IS the canonical programmatic API for agents (not the Tauri command
     list); agent process model: app-native agents run in the core process (S6b), external
     worker CLIs run inside PTY sessions.
   - **Co-viewing plan of record (D5):** wire-level multi-subscriber attach (per-subscriber
     Replay); single-attach remains the S1 behavior until protocol v2 lands.
   - `waiting_for_input` honest scope: canonical-mode line-input heuristic ONLY; structurally
     blind to raw-mode TUIs (claude-code, opencode, …) — worker liveness/stuck detection is a
     **named S6d subsystem** (worker adapters) with per-CLI strategies, NOT this flag.
   - Missing agent capabilities recorded as planned additive requests: `ReadOutput{since_seq}`
     cursor read, rendered-text snapshot, command+argv spawn (no shell), typed exit-status wait.
4. **NEW «Data-layer charter» (A13/A25/A26/A27/D6):** daemon owns terminal-domain durable state
   ONLY; S3+ domain data (goals, kanban, graph) lives in a core-owned store chosen in the S3 spec;
   Project ⇄ Workspace: *a Project is the planning entity that owns goals/graph/kanban and maps
   1..N Workspaces (repo roots)* — one sentence, locked; S4 row expanded (storage decision, node
   identity, retrieval API, **S4 hard-blocks S6 per D6**); historical telemetry (worker transcript
   log + `command_events` table) lands at next daemon schema bump (Cycle 2 or S3).
5. **Roadmap re-decomposition (A8/A17/A26/A28/D6):**
   - Insert **«Protocol v2» slice** (Cycle 2): codec migration + version negotiation +
     multi-subscriber attach + real drain + schema-migration policy for `bpa.db`.
   - S6 → S6a provider layer (trait, OpenRouter/OpenAI/GLM adapters, routing/fallback, streaming,
     retries, per-call cost/token/latency capture from day one) / S6b agent runtime + ONE role
     end-to-end / S6c escalation loop + approval inbox UI (P1) / S6d external-worker adapters /
     S6e custom-agent authoring. Order: S4 → S6a → S6b → S6c → S6d → S6e.
   - S7 (observability) += LLM traces, spend/budgets, evals.
   - Every S2+ slice row gets: product DoD bullets (3–5) + north-star metric(s) (A28).

### 3.3 F3 — `docs/backlog.md` (NEW; schema locked)

Table schema: `ID | Severity | Area | Summary | Origin (finding/commit) | Owner slice | Status`.
Seed items (transcribed verbatim-in-substance; IDs BL-1…):
1. env_overrides DYLD_*/LD_* denylist in daemon (A11-impl; §16 decision).
2. Restrictive CSP in tauri.conf.json (A19-impl).
3. bpa.db file mode 0600 + purge-on-delete (A18-impl).
4. Session/workspace deletion + retention per P3 (A15-impl) + `SessionLimitReached` resource model (A21).
5. Reconnect stale-pane UX + visual degradation when disconnected (A22-impl).
6. Error-code→message surface, toasts, catch on all fire-and-forget call sites (A23-impl).
7. Daemon-restart rehydration e2e phase + persistence-degraded event (A12-impl).
8. Scrollback flush dirty-check + DB write-architecture review (A24-impl).
9. flush_scrollback_once per-tick lock batching (final-review deferral).
10. pty_supervisor sink MutexGuard held across send — hardening (final-review deferral).
11. Escaped-descendant stream: cancel reachable after remove_session; Output-after-ChildExited
    (truncation-fix verification #14).
12. Stale attach entry when child exits during in-flight attach Replay send (verification #15).
13. Preflight call-site wiring untested (verification #16).
14. Replay applied without xterm reset → duplicated scrollback on re-attach (verification #17;
    chip task_ada4835d).
15. Cross-layer stale-channel windows: 30 s-hang double-replay; unconditional remove_attachment
    on failed attach (attach-dedup verification).
16. Test-infra: singleton env-lock; attach parallel flake; real-shell tests need CI assertion
    they ran (ledger CI-TODO).
17. Coverage gate: install cargo-llvm-cov in CI, enforce ≥80 % sessiond (spec §14.3).
18. ~28 unused iOS/Android icon assets — prune (final-review deferral).
19. Tauri auto-updater channel (D4 roadmap).
20. macOS Keychain provider-key storage (S6a; §16).
Backlog rule (also in CONTRIBUTING): **any accepted-deferred finding MUST land here in the same
change that defers it.**

### 3.4 F4 — `docs/runbook-daemon.md` (NEW)

Sections: locations (socket dir, `bpa.db`, logs, LaunchAgent plist, per-session runtime dirs);
inspect (launchctl print, socket probe, log tail); restart (kickstart -k semantics + «live shells
end» warning); full reset (bootout, delete state, re-bootstrap); uninstall; DB quarantine
semantics (what triggers, where the quarantined file goes, how to recover); dev-mode vs installed
daemon differences + cleanup; log rotation posture (tracing-appender daily; size cap = backlog note).

### 3.5 F5 — `CONTRIBUTING.md` (NEW)

Dev setup (toolchain via rust-toolchain.toml, Node engines, `npm ci`, sessiond build); gates =
`scripts/final-suite.sh` (list all stages) and CI parity statement; TDD required + DoD checklist
(tests, error handling, docs same-change, **backlog rule D2**); planning cycle (brainstorm → spec
→ plan → subagent execution, specs/plans paths); commit conventions (conventional commits +
trailer); protocol change rules (append-only + never-re-derive box, link spec §3 amendment).

### 3.6 F6 — `docs/frontend-conventions.md` (NEW)

Zustand: slice-per-feature module boundaries for S2+ (terminal / workspaces / kanban / graph /
agents / settings), single store composition, NO PTY bytes in React state (locked — bytes go
straight to xterm instances owned by TerminalManager); event naming `<domain>://<event>` + a
subscription registry pattern; `daemon://connected` initial event note (documented gap: none is
emitted on first connect — hydrate-until-success is the contract, spec §6.3 cross-ref); design
tokens: extract from `theme.ts` (spacing/typography/radius as CSS variables), light-mode posture =
dark-only for v0.x (explicit); testing contract per slice: unit (store/logic) + component-
integration against real store/manager (the layer that would have caught A1) + one GUI smoke;
tooling pinned before S2 (named in plan).

### 3.7 F7–F10 — CI & reproducibility

- **`.github/workflows/ci.yml`:** trigger push/PR on `main`; single `macos-14` job (arm64), steps:
  checkout → rust toolchain from `rust-toolchain.toml` + cache → `cargo fmt --check` →
  `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` → Node setup
  (engines) + `npm ci` → `npx tsc --noEmit` → `npx vitest run` → ts-rs parity (regen + git diff
  --exit-code) → `cargo build -p bpa-sessiond` + `npm run e2e:survive` → real-shell-tests-ran
  assertion (grep test output for the shell_integration test names; fail if skipped). Coverage: a
  SECOND job `coverage` (same runner class) installs cargo-llvm-cov and enforces the ≥80 %
  sessiond gate via `scripts/coverage-gate.sh`, blocking (`continue-on-error: false`) — this
  closes the never-executed gate for good; the plan may parallelize the two jobs but may not drop
  or soft-fail coverage (backlog item 17 then becomes «verified in CI», local run stays optional).
  Action versions verified current at plan time (Context7/web).
- **`rust-toolchain.toml`:** `channel = "1.92"` + components rustfmt, clippy.
- **`package.json`:** `engines.node` pinned to the major in use (verify at plan time).
- **`CHANGELOG.md`:** Keep-a-Changelog format; seed `[0.1.0] – S0+S1 foundation + terminal core`.
- **`scripts/final-suite.sh`:** add fmt + tsc stages → 8 stages, CI parity.
- **GitHub:** `gh repo create sshlg/builder-pro-ai --private --source . --push` (active gh account
  verified: API login `sshlg`; if creation under that owner is refused, fall back to the other
  authenticated account `ssheleg` and record which); push `main` + the fix-pass branch; verify a
  live CI run green; README badge.

### 3.8 F11–F14 — truth passes (README, architecture.md, traceability.md, build-macos.md)

Each: correct the specific false/stale claims enumerated by findings A12/A24/A30 (README quickstart
missing sessiond build step; architecture.md «no protocol drift is possible» + missing dual-codec
+ module map incl. `bpa-paths`; traceability e2e-scope wording; build-macos workspace member list,
release security posture statement, smoke-VM prereqs). The plan enumerates exact line edits from
the audit JSON (`evidence` fields carry file:line).

## 4. Traceability: finding → deliverable

- A2→F1.1+F12; A3→F2.2; A4/A5/A6/A16→F2.3; A7/A9/A11/A18/A19→F1.5; A8/A17/A28→F2.5; A10→F3+F5;
  A12→F1.7+F11+F13; A13/A25/A26/A27→F2.4; A14→F2.2; A15→F1.5(P3)+F3; A21→F1.6+F3+F12;
  A20/A31→F7–F10+F15; A22/A23→F1.4+F3; A24→F1.3+F12; A29→F4; A30→F1.2+F11+F12+F14; A32→F6.
- A1 — already fixed in code (`main` @ 285cb2e); F1.4 documents the contract.
- Plan self-check: every one of A2–A32 appears in ≥1 task's DoD.

## 5. Execution shape (input to writing-plans)

- Isolated worktree branch `worktree-docs-truth-ci` off `main`.
- ~14 tasks; **file-ownership map §2 is the parallel-safety contract** (no two parallel tasks touch
  one file). Suggested groups: G1 sequential foundation (F3 backlog + F5 CONTRIBUTING — process
  docs others cross-link); G2 parallel (F1, F2, F4, F6, F11–F14); G3 sequential CI (F7–F10, F15);
  G4 GitHub activation + live CI; G5 final whole-branch docs review (cross-link/consistency/
  truth spot-checks vs code) + merge.
- Every task: verifiable DoD (finding IDs closed + `bash scripts/final-suite.sh` unaffected/green
  where relevant + markdown link check for edited files).
- Two-stage review per task (spec compliance → quality), as established.

## 6. Verification / Definition of Done (cycle level)

1. All findings A2–A32 traceable to landed edits (final review checks the §4 matrix).
2. `scripts/final-suite.sh` (8 stages) green locally at branch tip.
3. GitHub repo exists (private), `main` + branch pushed, **live CI run green**.
4. No doc contradicts the code at `main` (final review samples every amended claim against source).
5. Backlog rule live: CONTRIBUTING states it; `docs/backlog.md` seeded with 20 items.
6. Gitignored ledgers contain nothing durable that is not also in tracked docs.

## 7. Human steps

**None.** (`gh` is authenticated with repo scope; CI activation is autonomous per P4.
Pre-existing documented human steps — notarized build T24, coverage local run — remain tracked in
backlog/build docs, unchanged by this cycle.)

## 8. Cycle 2 seed (recorded, out of scope here)

Protocol v2 spec will cover: codec selection (postcard vs bincode2 — Context7 doc verification
required before locking), version-range negotiation, multi-subscriber attach (D5), real
`DaemonShutdown{drain}`, `bpa.db` schema-migration policy + `command_events`, daemon-upgrade
consent flow (D4). Its brainstorm starts from this spec's §3.2(2) policy section.
