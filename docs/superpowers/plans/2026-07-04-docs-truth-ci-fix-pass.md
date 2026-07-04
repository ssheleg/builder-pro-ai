# Docs Truth + CI Fix Pass (Cycle 1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every tracked doc tell the truth about the built system, lock S2–S8 forward contracts at roadmap level, make the deferred backlog durable, and machine-enforce all quality gates via GitHub Actions CI.

**Architecture:** Pure docs + CI-config pass on a worktree branch off `main` (@ `5e5db13`). No product code changes except one mechanical `cargo fmt` normalization commit. Spec: [`docs/superpowers/specs/2026-07-04-docs-truth-ci-fix-pass-design.md`](../specs/2026-07-04-docs-truth-ci-fix-pass-design.md) (all §3 content requirements are copied into tasks below — implementers do NOT need the spec open). Audit evidence: [`docs/superpowers/research/2026-07-03-docs-spec-audit.json`](../research/2026-07-03-docs-spec-audit.json).

**Tech Stack:** Markdown; GitHub Actions (`macos-15` runner, `actions/checkout@v6`, `actions/setup-node@v6`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@v2`); rustfmt/clippy 1.92; Node 24 (CI) / `engines >=24`.

## Global Constraints

- Branch: `worktree-docs-truth-ci` off `main` @ `5e5db13`. Worktree via superpowers:using-git-worktrees.
- `cargo` lives at `$HOME/.cargo/bin` — every shell: `export PATH="$HOME/.cargo/bin:$PATH"`.
- **No product code changes.** Only files listed in a task's **Files** block may change. Exception: Task 12's one-time `cargo fmt` normalization (mechanical; full test suite proves semantics unchanged).
- Conventional commits; EVERY commit ends with trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Commit messages for doc edits cite the finding IDs they close (e.g. `docs(spec): codec truth pass (A2, A30)`).
- Line numbers below were captured at `5e5db13`; ALWAYS re-locate by grepping the quoted text, never edit blind by line number.
- Locked cross-task strings (spelled exactly):
  - Overview new section headings: `## Protocol evolution & upgrade policy`, `## Data-layer charter`
  - Spec changelog heading: `## Post-implementation amendments (2026-07-04)`
  - Backlog IDs `BL-1` … `BL-20`; file `docs/backlog.md`
  - Contract box text: `DO NOT re-derive Serialize/Deserialize on SessionLifecycle or TerminalEvent, and DO NOT add new serde-tagged enums to the Hop-B protocol, until protocol v2 replaces the codec.`
- Verification helper used by several tasks (relative markdown-link check; run from repo root):
  ```bash
  check_links() { f="$1"; d=$(dirname "$f"); grep -oE '\]\((\.\.?/[^)#]+|[A-Za-z0-9_./-]+\.md)' "$f" | sed 's/](//' | while read -r p; do [ -e "$d/$p" ] || [ -e "$p" ] || echo "BROKEN in $f: $p"; done; }
  ```
  Expected output: empty (no `BROKEN` lines).
- Truth-table canonical wording (used by Tasks 3, 4, 7 verbatim — single source):
  > **GUI close / crash / restart:** live shells **keep running** (daemon-owned); reattach + replay.
  > **Daemon restart / upgrade / crash:** live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions.

---

### Task 1: `docs/backlog.md` — canonical durable backlog

**Files:**
- Create: `docs/backlog.md`

**Interfaces:**
- Produces: backlog IDs `BL-1`…`BL-20` referenced by Tasks 2, 3, 4, 8, 9. Table schema: `ID | Severity | Area | Summary | Origin | Owner slice | Status`.

- [ ] **Step 1: Write the file** with this exact structure and all 20 seed rows:

```markdown
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
```

- [ ] **Step 2: Verify**

Run: `grep -c '^| BL-' docs/backlog.md`
Expected: `20`

- [ ] **Step 3: Commit**

```bash
git add docs/backlog.md
git commit -m "docs: seed canonical durable backlog with 20 deferred items (A10)"
```

---

### Task 2: `CONTRIBUTING.md` — process rules

**Files:**
- Create: `CONTRIBUTING.md`

**Interfaces:**
- Consumes: `docs/backlog.md` (Task 1) — cross-links it.
- Produces: the normative backlog rule quoted by Task 4's overview §6 edit; gates list consumed by Task 7 (README cross-link).

- [ ] **Step 1: Write the file** covering, in this order (each section 5–15 lines, concrete):
  1. **Dev setup:** toolchain auto-pinned by `rust-toolchain.toml` (rustup honors it); Node `>=24` (`package.json` engines); `npm ci`; daemon dev build `cargo build -p bpa-sessiond`.
  2. **Gates:** `bash scripts/final-suite.sh` runs all 8 stages (list them: rust tests, clippy `-D warnings`, fmt `--check`, TS tests, tsc `--noEmit`, ts-rs parity, coverage ≥80 % sessiond, e2e survive-restart); CI (`.github/workflows/ci.yml`) runs the same set — local and CI gates must never diverge.
  3. **TDD + DoD checklist:** failing test first; error handling + honest degradation; docs updated in the same change; **the backlog rule, verbatim:**
     > Any accepted-deferred finding MUST land in `docs/backlog.md` in the same change that defers it. Gitignored ledgers (`.superpowers/…`) are working notes, never the record.
  4. **Planning cycle:** brainstorm → spec (`docs/superpowers/specs/`) → plan (`docs/superpowers/plans/`) → subagent-driven execution with two-stage review.
  5. **Commit conventions:** conventional commits; agent trailer line.
  6. **Protocol change rules:** append-only wire discipline; the locked contract box (Global Constraints, verbatim); link `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` §3 amendment and `docs/architecture.md`.

- [ ] **Step 2: Verify**

Run: `grep -c "docs/backlog.md" CONTRIBUTING.md` → Expected: ≥2. Run the `check_links` helper on `CONTRIBUTING.md` → no BROKEN lines.

- [ ] **Step 3: Commit**

```bash
git add CONTRIBUTING.md
git commit -m "docs: CONTRIBUTING with gates, TDD/DoD, backlog rule, protocol change rules (A10)"
```

---

### Task 3: S0+S1 spec amendments (in place)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`

**Interfaces:**
- Consumes: `BL-*` ids (Task 1). Produces: `## Post-implementation amendments (2026-07-04)` section + amended §3/§5/§11/§12/§13/§15/§16 that Tasks 7, 8 cross-reference.

All seven amendments below. Insert the changelog section right after the doc's title block; each amendment adds a row.

- [ ] **Step 1: Add changelog section** after the title/date block:

```markdown
## Post-implementation amendments (2026-07-04)

The S0+S1 implementation (merged @ 285cb2e) diverged from this spec in specific,
audited ways. The sections below were amended IN PLACE so this document remains the
single executable truth for zero-context implementers. Full audit:
`docs/superpowers/research/2026-07-03-docs-spec-audit.json`.

| Amendment | Sections | Finding |
|-----------|----------|---------|
| Codec truth: dual-codec bridge + never-re-derive contract | §3, §5, §7 | A2 |
| §15 verify-at-lock resolution table | §15 | A2/A30 |
| Persistence truth: 1000 ms cadence, no size trigger, seq-0 semantics, 256 KiB cap | §11 | A24 |
| Attach/reconnect + error-surfacing contracts | §12, §13 | A22/A23 |
| Trust model rewrite: agent trust layer, keys, CSP, data-at-rest, env_overrides | §16 | A7/A9/A11/A18/A19 |
| Resource model honest posture + planned cap | §9, §13 | A21 |
| Survival truth wording | §13 | A12 |
```

- [ ] **Step 2: Codec truth (A2).** Locate `serde-native, deterministic fixint LE` (~line 95) and the `**Codec decision:**` paragraph (~109–112). Rewrite the §3 codec row to `bincode 1.3.3 (fixint LE framing; tagged enums via hand-written dual-codec impls — see below)` and REPLACE the codec-decision paragraph with:

```markdown
**Codec decision (amended 2026-07-04, A2):** `bincode` **1.3.3**. As built, bincode 1.3
CANNOT deserialize serde internally/adjacently-tagged enums. `SessionLifecycle` and
`TerminalEvent` therefore ship hand-written `Serialize`/`Deserialize` impls that branch on
`is_human_readable()`: JSON (and ts-rs) see the tagged shape; bincode tunnels a JSON string
inside the binary frame (dual codec). See `crates/protocol/src/lib.rs` (dual-codec note) and
`crates/protocol/tests/roundtrip.rs` (every variant round-trips both ways).

> **Locked contract:** DO NOT re-derive Serialize/Deserialize on SessionLifecycle or
> TerminalEvent, and DO NOT add new serde-tagged enums to the Hop-B protocol, until
> protocol v2 replaces the codec.

Trajectory (owner decision D3): protocol v2 (Cycle 2) migrates to a tagged-enum-safe codec
(postcard or bincode 2 serde-compat — decided in the Cycle 2 spec) bundled with version
negotiation; the bridge and this contract box then retire.
```

In §5, above the `SessionLifecycle`/`TerminalEvent` enum listings (derives at ~181–195), add one line each: `<!-- As built: hand-written serde impls (dual codec, §3); the derive shown is the LOGICAL shape ts-rs exports. -->` and keep the listings.

- [ ] **Step 3: §15 resolution table (A2/A30).** Append to §15 a table `| §15 item | Resolution |` with one row per §15 bullet; minimum required rows: 15.5 codec → `DIVERGED — see §3 amendment (dual codec)`; 15.3 binary Channel payload → state what was actually built (grep `src-tauri/src/broker.rs` for the `TerminalEvent` channel: events are serialized via serde/JSON through Tauri's Channel — record the real mechanism found); every other item → `verified as specced` or the concrete divergence.

- [ ] **Step 4: Persistence truth (A24).** In §11 (~line 607–609): replace `every ≈500 ms or 32 KB per session` with `every 1000 ms (SCROLLBACK_FLUSH_INTERVAL) per session; there is NO size-based trigger`; replace the loss-window sentence with `bounded by the flush interval (≤ ~1 s of tail output)`; add: `Replay/persist cap: 256 KiB per session ring (locked constant SCROLLBACK_CAP — verify the name by grepping crates/sessiond/src/scrollback.rs and cite the real identifier); snapshot-replace semantics: each sweep REPLACES the stored blob at seq 0 (no append log). Dirty-check + write-architecture review: BL-8.`

- [ ] **Step 5: Attach/reconnect + error contracts (A22/A23).** In §12 add subsection `### Attach state contract (as built @ 4c91dfe)`: per-session state machine `detached | attaching | attached` owned by `TerminalManager`; attach coalescing (concurrent callers share one in-flight promise, one IPC); generation guard invalidates stale completions on reset/dispose; reconnect: `daemon://reconnected` → reset ALL attach state → eager re-attach visible session → hidden sessions re-attach lazily on next show with fresh Replay; stale panes visually marked until re-attached (**implementation BL-5**). In §13 add subsection `### Error-surfacing contract (normative; implementation BL-6)` with the invariant `No command rejection is silent` and a table mapping every `CommandError` kind + daemon error code (enumerate them by grepping `src-tauri/src/commands.rs` for `CommandError` and `crates/sessiond/src/socket_server.rs` for `code:` strings — the table must list every real code found: at minimum `Disconnected`, `Internal`, `NoSuchSession`, `InvalidWorkspaceRoot`, `CwdMissing`, `RelativePath`, `SymlinkEscape`, `NotADirectory`, `InvalidShell`, `SpawnError`, `PtyError`, `IoError`, `DbError`, `SinkClosed`, `UnexpectedHello`) → one user-facing message sentence each.

- [ ] **Step 6: §16 trust-model rewrite (A7/A9/A11/A18/A19 + P1/P2).** Replace §16's body with a consolidated model that KEEPS all still-true current content (peer-cred gate, path validation via shared `bpa-paths`, OSC-forgeability warning, DAEMON_SECRET hygiene — verify each still matches code before carrying it over) and ADDS these subsections with this normative content:
  - `### Agent execution trust layer (roadmap — implemented in S6c)`: approval-gate classes: destructive-fs outside workspace, `git push --force`, package publish, spend/payment, production deploy. Batched approvals via the escalation inbox; synchronous confirm only for irreversibly destructive actions. Append-only audit log of every agent-initiated command. Caller identity: same-uid peer-cred today; per-agent identity tokens are an S6 design item.
  - `### SSH agent`: SSH_AUTH_SOCK is withheld by default from agent-spawned sessions, grantable per-task via approval; human GUI sessions keep current behavior.
  - `### env_overrides`: decision — the daemon WILL enforce a `DYLD_*`/`LD_*` denylist (BL-1); until it lands, the §9.3 allowlist is a default, not a ceiling (stated honestly).
  - `### Provider API keys`: macOS Keychain only (BL-20); never SQLite/config/logs; LLM egress from the core process only — the webview never talks to model providers.
  - `### Webview boundary`: target = restrictive CSP in `tauri.conf.json` (BL-2); current state `csp: null`, stated honestly until BL-2 lands.
  - `### Data at rest`: `bpa.db` contains raw terminal scrollback ⇒ secret-bearing; 0600 file mode (BL-3); purge-on-delete ties into retention (BL-4); retention defaults: keep last 20 exited sessions per workspace / 30-day TTL (config later); workspace delete cascades live sessions with consent.

- [ ] **Step 7: Resource model (A21).** In §9 (threading contract) add: `Per-session cost envelope: 3 OS threads (reader/wait/ticker) + 1 forwarder thread per live attachment + PTY fds. There is currently NO enforced cap on concurrent sessions; exhaustion behavior is the OS's. Planned additive remedy: configurable cap + typed SessionLimitReached error (BL-4).`

- [ ] **Step 8: Survival truth (A12).** Locate the §13 truth-table copy (~line 674) and replace the `Daemon restart | survive …` row with the canonical wording from Global Constraints. Add to the §13 degradation matrix a row: `persistence-degraded (quarantine/flush-failure) Push event → UI indicator — specced future event, BL-7`.

- [ ] **Step 9: Verify**

```bash
grep -c "Post-implementation amendments" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md   # 1
grep -c "serde-native" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md                      # 0
grep -c "32 KB per session" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md                 # 0
grep -c "DO NOT re-derive" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md                  # ≥1
grep -c "SessionLimitReached" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md               # ≥1
grep -c "live shells .*end" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md                 # ≥1 (case-insensitive ok)
```

- [ ] **Step 10: Commit**

```bash
git add docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md
git commit -m "docs(spec): in-place truth amendments — codec, persistence, contracts, §16, survival (A2,A12,A21,A22,A23,A24,A7,A9,A11,A18,A19)"
```

---

### Task 4: Overview — new sections + roadmap re-decomposition

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md`

**Interfaces:**
- Consumes: BL ids; locked headings (Global Constraints). Produces: `## Protocol evolution & upgrade policy` and `## Data-layer charter` sections cross-linked by Tasks 7/8; the re-decomposed roadmap consumed by every future slice spec.

- [ ] **Step 1: Truth table (A12).** Replace rows ~51–52 with the canonical wording (Global Constraints), and ADD row: `| Agent runs (S6+) | undefined until S6b — agent state is NOT covered by the daemon survival model (honest placeholder) |`.

- [ ] **Step 2: `## Protocol evolution & upgrade policy` (A3/A14; D3/D4).** New section after §2 with EXACTLY these subsections: **Wire discipline** (append-only variant order frozen, additive fields only; cross-version decode tests are a standing requirement for every protocol change); **Version negotiation** (client sends supported range; daemon answers in-range or `Incompatible{min,max}` with a remediation dialog; implemented in protocol v2/Cycle 2 alongside the codec migration — postcard or bincode 2 serde-compat, decided there); **Daemon upgrade choreography (owner decision D4)** (consent dialog: «Update background service — N live sessions will end; records+scrollback survive», real drain via SIGTERM then `launchctl kickstart -k`; `DaemonShutdown{drain}` is currently a no-op Ack and is **reserved** until protocol v2 defines real drain); **Release channel** (manual notarized DMG now; Tauri auto-updater = BL-19); **Workspace evolution (A14)** (multi-root workspaces arrive as additive `workspace_roots: Vec<PathBuf>` alongside `root_path`, slice S2).

- [ ] **Step 3: §4 rewrite (A4/A5/A6/A16; D5).** Replace §4's body: Hop-B socket protocol IS the canonical programmatic agent API (not the Tauri command list); agent process model: app-native agents (CEO/PM/eng) run in the core process (S6b), external worker CLIs run inside PTY sessions; **co-viewing plan of record (D5): wire-level multi-subscriber attach** (N subscribers per session, each with its own Replay) designed+implemented in protocol v2 — single-attach remains S1 behavior until then; `waiting_for_input` honest scope: canonical-mode line-input heuristic ONLY, structurally blind to raw-mode TUIs (claude-code, opencode, …) — worker liveness/stuck detection is the **S6d worker-adapter subsystem** with per-CLI strategies, NOT this flag; planned additive agent capabilities: `ReadOutput{since_seq}` cursor read, rendered-text snapshot, command+argv spawn (no shell), typed exit-status wait.

- [ ] **Step 4: `## Data-layer charter` (A13/A25/A26/A27; D6).** New section: daemon owns terminal-domain durable state ONLY; S3+ domain data (goals, kanban, knowledge graph) lives in a core-owned store chosen in the S3 spec; **Project ⇄ Workspace (locked):** `A Project is the planning entity that owns goals/graph/kanban and maps to 1..N Workspaces (repo roots).`; S4 expanded: storage decision, UUID node identity, agent retrieval API are S4-spec items, and **S4 hard-blocks S6 (owner decision D6)**; historical telemetry (worker transcript log + `command_events` table) lands at the next daemon schema bump (protocol v2 or S3); fix the stale S0 storage row if present (grep `local SQLite` in §3 and true it up against `crates/sessiond/src/persistence.rs`).

- [ ] **Step 5: Roadmap re-decomposition (A8/A17/A26/A28; D6).** In §3's table: insert slice row **`Protocol v2`** (codec migration + version negotiation + multi-subscriber attach + real drain + bpa.db schema-migration policy; position: immediately after S1); split S6 into **S6a** LLM provider layer (trait, OpenRouter/OpenAI/GLM adapters, routing/fallback, streaming, retries, per-call cost/token/latency capture from the first call), **S6b** agent runtime + ONE role end-to-end, **S6c** escalation loop + approval inbox UI, **S6d** external-worker adapters, **S6e** custom-agent authoring; dependency order `S4 → S6a → S6b → S6c → S6d → S6e`; S7 gains LLM traces, spend/budgets, evals; EVERY S2+ row gets 3–5 product-DoD bullets and ≥1 north-star metric (write them — e.g. S2 workspaces: «create/open/switch a multi-repo workspace in <3 clicks», metric: time-to-first-terminal; use concrete, testable phrasing for each slice).

- [ ] **Step 6: Verify**

```bash
grep -c "## Protocol evolution & upgrade policy" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # 1
grep -c "## Data-layer charter" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md                   # 1
grep -c "S6a" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md                                     # ≥2
grep -c "multi-subscriber" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md                        # ≥1
grep -c "≈500 ms" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md                                 # 0
grep -ci "S4 hard-blocks S6\|S4 (knowledge graph) is a hard prerequisite" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # ≥1
```

- [ ] **Step 7: Commit**

```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): protocol-evolution policy, agent forward contracts, data charter, S6a-e roadmap (A3,A4,A5,A6,A8,A13,A14,A16,A17,A25,A26,A27,A28)"
```

---

### Task 5: `docs/runbook-daemon.md`

**Files:**
- Create: `docs/runbook-daemon.md`

**Interfaces:** standalone; Task 7 links it from README.

- [ ] **Step 1: Gather REAL values from code** (never invent): `grep -n "Label\|plist\|LaunchAgents" src-tauri/src/launchd.rs` (LaunchAgent label + plist path); `grep -n "socket_dir\|XDG_RUNTIME_DIR\|bpa" crates/sessiond/src/boot.rs src-tauri/src/paths.rs | head -20` (socket + state dirs); `grep -n "tracing_appender\|log" crates/sessiond/src/logging.rs | head` (log location + rotation); `grep -n "quarantine" crates/sessiond/src/persistence.rs | head` (quarantine semantics).

- [ ] **Step 2: Write the runbook** with sections: **Locations** (table: socket, `bpa.db`, logs, plist, per-session runtime dirs — real paths from Step 1, with `$HOME`/`$XDG_RUNTIME_DIR` variables); **Inspect** (`launchctl print gui/$(id -u)/<label>`, socket liveness probe, log tail commands); **Restart** (`launchctl kickstart -k gui/$(id -u)/<label>` + WARNING box: live shells end, records+scrollback rehydrate inactive); **Full reset** (bootout → delete state dir → re-bootstrap; exact commands); **Uninstall** (bootout + rm plist + rm state; note in-app uninstall is future work); **DB quarantine** (what triggers it, where the file goes, how to recover — from Step 1 grep); **Dev mode vs installed** (dev spawns `target/debug/bpa-sessiond` with isolated `XDG_RUNTIME_DIR` — confirm against `tests/e2e/lib/daemon-harness.mjs` and `src-tauri/src/lib.rs`); **Log rotation** (tracing-appender daily rotation as built; size cap = note, no BL needed).

- [ ] **Step 3: Verify** — every `launchctl` command uses the REAL label from Step 1 (`grep -c "<real-label>" docs/runbook-daemon.md` ≥3); `check_links docs/runbook-daemon.md` → empty.

- [ ] **Step 4: Commit**

```bash
git add docs/runbook-daemon.md
git commit -m "docs: daemon operations runbook — inspect/restart/reset/uninstall/quarantine (A29)"
```

---

### Task 6: `docs/frontend-conventions.md`

**Files:**
- Create: `docs/frontend-conventions.md`

**Interfaces:** standalone; consumed by every S2+ frontend task.

- [ ] **Step 1: Write** with these sections and normative content:
  1. **Store architecture:** Zustand slice-per-feature for S2+ (`terminal`, `workspaces`, `kanban`, `graph`, `agents`, `settings`), composed into one store; **locked invariant: PTY bytes NEVER enter React state** — the firehose goes straight into xterm instances owned by `TerminalManager` outside Zustand (cite `src/terminal/terminal-manager.ts`).
  2. **Events:** naming rule `<domain>://<event>` (existing: `daemon://disconnected`, `daemon://reconnected`, `session://created|exited`, …— enumerate the real set by grepping `src/ipc/events.ts`); one subscription-registry module per slice; documented gap: NO `daemon://connected` fires on first connect — hydrate-until-success is the contract (cite `src/App.tsx` doc comment).
  3. **Attach state:** cross-reference spec §12 attach contract (Task 3 Step 5) — per-session state machine lives in `TerminalManager`; panes call `attach()` unconditionally, manager dedupes.
  4. **Design tokens:** extract from `src/theme.ts` (list its real exports by reading the file) as the token source; spacing/typography/radius as CSS variables at next UI slice; dark-only for v0.x (explicit posture).
  5. **Testing contract per slice:** unit (store/logic) + component-integration against the REAL store/manager (the layer that caught A1) + one GUI smoke; tooling pinned before S2.

- [ ] **Step 2: Verify** — `grep -c "NEVER enter React state\|never enter React state" docs/frontend-conventions.md` ≥1; `check_links docs/frontend-conventions.md` → empty.

- [ ] **Step 3: Commit**

```bash
git add docs/frontend-conventions.md
git commit -m "docs: frontend platform conventions — slices, events, tokens, test layers (A32)"
```

---

### Task 7: README truth pass

**Files:**
- Modify: `README.md`

**Interfaces:** Consumes canonical truth wording (Global Constraints), runbook (Task 5), CONTRIBUTING (Task 2). Task 14 adds the CI badge later — leave line 1 alone.

- [ ] **Step 1: Survival wording (A12).** Locate `What *does* survive — GUI restarts and daemon restarts — is proven end-to-end by \`npm run e2e:survive\`` (~line 76–77). Replace with: `What *does* survive — GUI restart with live shells, and daemon restart for records+scrollback (rehydrated as inactive) — is stated in the truth table above. `npm run e2e:survive` proves the client-restart half end-to-end (daemon-restart rehydration e2e: see docs/backlog.md BL-7).` Check the truth table above it (~line 71) matches the canonical wording; fix any `daemon restarts` overclaim.
- [ ] **Step 2: Quickstart (A30).** Follow the quickstart block yourself from a clean state mentally against the code: verify whether `npm run tauri dev` really spawns the daemon dev build automatically (grep `src-tauri/src/lib.rs` for the dev-mode daemon spawn path). If a manual `cargo build -p bpa-sessiond` is required first, add that step; if not, correct whatever IS wrong per audit A30 evidence (the audit found the quickstart broken — reproduce the exact gap and fix it truthfully).
- [ ] **Step 3: Gates section.** List all 8 final-suite stages (post-Task 12) + link `CONTRIBUTING.md` and `docs/runbook-daemon.md`.
- [ ] **Step 4: Verify** — `grep -c "BL-7" README.md` ≥1; `check_links README.md` → empty (ignore the not-yet-existing CI badge).
- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs(readme): survival truth, quickstart fix, gates list (A12, A30)"
```

---

### Task 8: `docs/architecture.md` truth pass

**Files:**
- Modify: `docs/architecture.md`

- [ ] **Step 1: Codec truth (A2).** Locate `no protocol drift is possible because both sides import the exact same enum` (~line 54–57). Replace the claim with: shared `bpa-protocol` crate prevents *type* drift; the wire codec is bincode 1.3.3 with a dual-codec bridge for tagged enums (`SessionLifecycle`, `TerminalEvent` — hand-written impls, JSON-in-bincode) + the locked never-re-derive contract box (Global Constraints, verbatim) + pointer to spec §3 amendment.
- [ ] **Step 2: Module map.** Add `crates/paths` (`bpa-paths`, shared core+daemon path validation, spec §16) to the module map; verify every other module listed still exists (`ls crates src-tauri/src src`).
- [ ] **Step 3: Resource envelope (A21).** Add: per-session cost = 3 OS threads + 1 forwarder per attachment + PTY fds; no enforced session cap today (BL-4).
- [ ] **Step 4: Verify** — `grep -c "no protocol drift is possible" docs/architecture.md` → 0; `grep -c "bpa-paths" docs/architecture.md` ≥1; `grep -c "DO NOT re-derive" docs/architecture.md` ≥1.
- [ ] **Step 5: Commit**

```bash
git add docs/architecture.md
git commit -m "docs(architecture): dual-codec truth, bpa-paths module, resource envelope (A2, A21, A30)"
```

---

### Task 9: `docs/traceability.md` corrections

**Files:**
- Modify: `docs/traceability.md`

- [ ] **Step 1: Fix the fabricated e2e claim (A12).** Row ~37 claims `npm run e2e:survive (phase4: real daemon-restart rehydrate, end-to-end)` — phase 4 actually restarts the CLIENT (daemon and shell survive a client quit; see `tests/e2e/survive-restart.mjs`). Rewrite: `… + npm run e2e:survive (phase4: client-quit survival + reattach; daemon-restart rehydration e2e is BL-7)`. Row ~55: same correction for the `daemon restart survives via rehydrate … proven end-to-end` half — proven at the persistence-unit level (`committed_rows_survive_reopen`), NOT by the e2e; say so and cite BL-7.
- [ ] **Step 2: Sweep** every other `e2e:survive` mention (`grep -n "e2e:survive" docs/traceability.md`) — each must describe only what phases 0–4 actually prove (handshake, create, attach+output, OSC lifecycle, client-quit survival + reattach replay).
- [ ] **Step 3: Verify** — `grep -c "real daemon-restart rehydrate" docs/traceability.md` → 0; `grep -c "BL-7" docs/traceability.md` ≥2.
- [ ] **Step 4: Commit**

```bash
git add docs/traceability.md
git commit -m "docs(traceability): true up e2e scope claims — phase4 is client-restart, daemon-restart rehydrate is BL-7 (A12)"
```

---

### Task 10: `docs/build-macos.md` truth pass

**Files:**
- Modify: `docs/build-macos.md`

- [ ] **Step 1:** Add `crates/paths` (`bpa-paths`) wherever workspace members are enumerated (grep `crates/` in the file; ~line 39 and ~315 context). Verify the list against the root `Cargo.toml` `members`.
- [ ] **Step 2: Release security posture (A30/A31).** Add a short section: hardened runtime + entitlements + notarization are REQUIRED for every distributed build (never ship unsigned); signing identity/team + App Store Connect key are the only human-held secrets; CI builds are test-only (unsigned) — release builds run `scripts/build-universal.sh` + `scripts/sign-verify.sh` locally.
- [ ] **Step 3: Smoke-VM prereqs.** Audit A30 flagged `scripts/smoke-clean-vm.sh` prerequisites as stale — read the script header, list its REAL prerequisites in the doc (VM image, env vars like `BPA_E2E_EXTERNAL_DAEMON=1`).
- [ ] **Step 4: Verify** — `grep -c "bpa-paths" docs/build-macos.md` ≥1; `check_links docs/build-macos.md` → empty.
- [ ] **Step 5: Commit**

```bash
git add docs/build-macos.md
git commit -m "docs(build): bpa-paths member, release security posture, smoke-VM prereqs (A30, A31)"
```

---

### Task 11: Reproducibility pins — `rust-toolchain.toml`, `engines`, `CHANGELOG.md`

**Files:**
- Create: `rust-toolchain.toml`
- Create: `CHANGELOG.md`
- Modify: `package.json` (engines field only)

**Interfaces:** Produces the toolchain pin Task 13's CI relies on.

- [ ] **Step 1:** Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.92"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2:** Verify the pin matches reality: `PATH="$HOME/.cargo/bin:$PATH" cargo --version` → expect `cargo 1.92.x`. Then `cargo test -p bpa-protocol --lib` (quick smoke that the pinned toolchain resolves) → green.
- [ ] **Step 3:** In `package.json` add (top level, after `"version"`): `"engines": { "node": ">=24" }` — verified at plan time: dev machine runs Node 25 (satisfies), CI uses Node 24 LTS.
- [ ] **Step 4:** Create `CHANGELOG.md` (Keep-a-Changelog format) seeded:

```markdown
# Changelog

All notable changes to Builder Pro AI. Format: keepachangelog.com; versioning: semver.

## [0.1.0] — 2026-07-04

### Added
- S0+S1 foundation + terminal core: launchd-managed `bpa-sessiond` daemon owning PTYs
  (survive-GUI-restart), OSC-133/7 shell integration, sanitized scrollback replay,
  SQLite persistence, React/xterm.js frontend with per-session attach state machine.
- Shared `bpa-protocol` + `bpa-paths` crates; ts-rs generated TS types (diff-gated).
- Gates: workspace tests, clippy -D warnings, vitest, ts-rs parity, e2e survive-restart.
```

- [ ] **Step 5:** `npm ci` (or `npm install --dry-run`) → no engines error. Commit:

```bash
git add rust-toolchain.toml CHANGELOG.md package.json
git commit -m "chore: pin rust 1.92 + node engines, seed CHANGELOG (A31)"
```

---

### Task 12: rustfmt normalization + final-suite gains fmt & tsc stages

**Files:**
- Modify: every `*.rs` under `crates/` and `src-tauri/src/` (mechanical `cargo fmt` only — 236 hunks across ~20 files at plan time)
- Modify: `scripts/final-suite.sh`

**Interfaces:** Produces the 8-stage gate list consumed by Tasks 2 (CONTRIBUTING), 7 (README), 13 (CI parity).

- [ ] **Step 1:** `export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt` (formats the workspace).
- [ ] **Step 2:** Prove semantics unchanged: `cargo test --workspace` → ALL green (expect 205+); `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
- [ ] **Step 3:** Commit formatting ALONE:

```bash
git add -u
git commit -m "style: one-time cargo fmt normalization (mechanical; suite green proves semantics unchanged)"
```

- [ ] **Step 4:** Edit `scripts/final-suite.sh`: insert after the clippy stage (currently `== 2/6 clippy ==`) a new stage `cargo fmt --check`; insert after the vitest stage a new stage `npx tsc --noEmit`; renumber all headers to `1/8 … 8/8` (order: rust tests, clippy, fmt --check, TS tests, tsc, ts-rs parity, coverage, e2e). Keep `set -euo pipefail` semantics — each stage is `command` + `echo OK`.
- [ ] **Step 5:** Close the never-executed coverage gate honestly: `PATH="$HOME/.cargo/bin:$PATH" cargo install cargo-llvm-cov --locked` (once), then run `bash scripts/final-suite.sh` END TO END. Expected: `ALL GATES PASSED` with the coverage stage printing the sessiond line-coverage ≥80 %. **If coverage < 80 %: STOP, report NEEDS_CONTEXT with the exact number and the uncovered files list** — the controller decides (test follow-up task vs gate adjustment with the owner). Do not lower the gate yourself. **If disk pressure blocks the instrumented build (needs ~3–5 GB): STOP, report BLOCKED with `df -h` output.**
- [ ] **Step 6:** Commit:

```bash
git add scripts/final-suite.sh
git commit -m "chore(gates): final-suite gains fmt --check and tsc --noEmit stages (8 total); coverage gate executed (A20, A31)"
```

---

### Task 13: `.github/workflows/ci.yml`

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:** Consumes `rust-toolchain.toml` (Task 11), 8-stage parity (Task 12). Produces the workflow Task 14 activates.

- [ ] **Step 1:** Write exactly (versions verified current 2026-07: runner `macos-15` = macos-latest since 2025-08; `checkout@v6`, `setup-node@v6`):

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:

jobs:
  gates:
    runs-on: macos-15
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable # rustup honors rust-toolchain.toml (1.92)
      - uses: Swatinem/rust-cache@v2
      - name: rustfmt
        run: cargo fmt --check
      - name: clippy (deny warnings)
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: rust tests
        run: cargo test --workspace
      - name: real-shell integration tests actually ran
        run: |
          out=$(cargo test -p bpa-sessiond shell_integration 2>&1)
          echo "$out"
          echo "$out" | grep -E "test .*shell_integration.* ok" >/dev/null
          ! echo "$out" | grep -qi "skip"
      - uses: actions/setup-node@v6
        with:
          node-version: 24
          cache: npm
      - run: npm ci
      - name: tsc
        run: npx tsc --noEmit
      - name: vitest
        run: npx vitest run
      - name: ts-rs parity
        run: |
          cargo test -p bpa-protocol --test ts_export
          git diff --exit-code -- src/ipc/types.ts
      - name: e2e survive-restart
        run: |
          cargo build -p bpa-sessiond
          npm run e2e:survive

  coverage:
    runs-on: macos-15
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-llvm-cov
      - name: coverage gate (sessiond >= 80%)
        run: bash scripts/coverage-gate.sh
```

- [ ] **Step 2:** Read `scripts/coverage-gate.sh` and confirm it (a) runs cargo-llvm-cov itself and (b) fails loudly below 80 % — if its interface differs (e.g. expects the tool pre-installed only), adapt the workflow step to call it correctly, NOT the script.
- [ ] **Step 3:** Adjust the real-shell assertion to the ACTUAL test names: `cargo test -p bpa-sessiond shell_integration -- --list 2>/dev/null | head` — the grep in Step 1 must match at least one real listed name; tighten the pattern to a specific known test fn (pick from the list output) so the assertion cannot vacuously pass.
- [ ] **Step 4:** Syntax check: `npx --yes yaml-lint .github/workflows/ci.yml` (or `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))"`) → parses clean.
- [ ] **Step 5:** Commit:

```bash
git add .github/workflows/ci.yml
git commit -m "ci: macOS workflow — all 8 gates + blocking coverage job (A20, A31)"
```

---

### Task 14: GitHub activation + live CI + README badge

**Files:**
- Modify: `README.md` (badge line only)
- External: GitHub repo creation, push

**Interfaces:** Consumes Task 13's workflow. Sequential — runs ONLY after all other tasks are merged into the branch.

- [ ] **Step 1:** `gh repo create sshlg/builder-pro-ai --private --source /Users/sshlg/DATA/builder-pro-ai --remote origin` (do NOT `--push` yet). If the owner is refused, retry with `ssheleg/builder-pro-ai` and record which one won in the task report.
- [ ] **Step 2:** Push: `git push -u origin main && git push -u origin worktree-docs-truth-ci`.
- [ ] **Step 3:** Watch the branch run: `gh run list --branch worktree-docs-truth-ci --limit 1` then `gh run watch <id> --exit-status`. Expected: **both jobs green.** If a step fails: fix ONLY within this plan's file scope (CI yaml, scripts, docs); a product-code failure = NEEDS_CONTEXT (report the failing step's log excerpt).
- [ ] **Step 4:** Add badge as README line 1: `![ci](https://github.com/<owner>/builder-pro-ai/actions/workflows/ci.yml/badge.svg)` (real owner from Step 1).
- [ ] **Step 5:** Commit + push:

```bash
git add README.md
git commit -m "docs(readme): CI badge (A20)"
git push
```

- [ ] **Step 6:** `gh run watch` the new run → green. Report run URL.

---

### Task 15: Cycle DoD sweep

**Files:** none created; verification + (if gaps found) fixes within prior tasks' files.

- [ ] **Step 1: Finding coverage.** For each of A2–A32, `git log --oneline main..HEAD | grep -c "A<n>"` OR locate the closing edit; produce a table in the task report mapping every finding → commit. A1 = pre-closed on main @ 285cb2e (note it). Any finding with no edit → fix now within the owning task's file.
- [ ] **Step 2: Banned-phrase sweep** (repo-wide, tracked files):

```bash
git grep -n "serde-native\|no protocol drift is possible\|32 KB per session\|real daemon-restart rehydrate" -- '*.md' && echo "FAIL: stale claims remain" || echo "OK"
git grep -n "≈500 ms" -- 'docs/superpowers/specs/*.md' && echo "FAIL" || echo "OK"
```

- [ ] **Step 3: Link check** — run `check_links` (Global Constraints) over every created/modified `.md` → no BROKEN lines.
- [ ] **Step 4:** `bash scripts/final-suite.sh` → `ALL GATES PASSED` (8 stages).
- [ ] **Step 5:** Ledger-vs-tracked check: confirm nothing durable exists ONLY in `.superpowers/` (spot-check the CONTRACT RULES + deferred lists against spec §3 amendment + `docs/backlog.md`).
- [ ] **Step 6: Commit** any sweep fixes: `docs: DoD sweep fixes (cycle 1 close-out)`.

---

## Execution notes (for the controller)

- **Groups:** G1 = T1→T2 (sequential; T2 links T1). G2 = T3, T4, T5, T6, T7, T8, T9, T10 (parallel-safe — disjoint files; T7 consumes the 8-stage list conceptually but only Task 12 edits the script, so T7 states the list from this plan). G3 = T11→T12→T13 (sequential). G4 = T14. G5 = T15 → final whole-branch review → merge via finishing-a-development-branch.
- **Review:** two-stage review per task (spec compliance → quality), final whole-branch docs review before merge (consistency + truth spot-checks against code).
- **Plan-time verifications done:** runner `macos-15`/`macos-latest`, `checkout@v6`, `setup-node@v6` (web, 2026-07); Node 25.5 local / 24 LTS CI; clippy/rustfmt 1.92 installed; repo NOT fmt-clean (236 hunks — Task 12 normalizes); `gh` authed (`sshlg`, repo scope); fabricated e2e claim confirmed at traceability.md:37.
```
