# Builder Pro AI — Platform Overview & Roadmap

**Date:** 2026-07-01
**Status:** Approved (decomposition + global decisions)
**Scope:** This is the *map*. Each subsystem below gets its own `spec → plan → implementation`
cycle. Detailed contracts live in per-slice specs, not here.

---

## 1. Product

> (amended 2026-07-06, vision v2–v4 — see `docs/superpowers/research/2026-07-06-product-vision-v2.md`)

A lightweight macOS control panel whose job is to **take any idea to a working project and
manage/organize the vibecoding process**. The pain it exists to kill: with 5–6 projects in flight
at different speeds and stages, goals and context get lost and the owner degrades into a poller
and button-presser — **the attention tax is the enemy**. Everything that can proceed under policy
proceeds without the owner; the system pulls the owner in (one batched inbox, morning summaries) —
the owner never polls.

Opening the app answers, for ALL projects at once and in under 30 seconds: where is each one,
what moved since I last looked, and what — if anything — needs ME. The home screen shows: a live
agent feed (who is doing what, on which stage, in which project), per-project progress,
idea quick-capture into a backlog, an Insights lane (market-sourced opportunities with
fit-verdicts against goals/metrics), agent-org settings (the CEO and the other organization
agents are configurable), vector steering (edit goals/priorities — agents pick it up), and the
hot-questions inbox. Underneath sit real terminals running off-the-shelf coding agents
(claude-code, hermes, opencode, kilo, …) driven by app-native meta-agents, plus workspaces of
repos, a live file explorer, a workspace-wide knowledge graph, a kanban, editable workflows, and
observability.

**North-star metrics**

1. **Time-to-context** on app open: < 30 s for all projects.
2. **Owner interventions per shipped task**: → 1 (the one approval that matters).
3. **Unattended progress**: hours agents advance without stalling on a button.

**Design tenets**

- **Production-grade, no MVP half-states.** Every slice ships finished: TDD tests, error handling
  + honest degradation, structured logging, docs. No "TODO later", no stubs.
- **Max autonomy, min human-in-the-loop.** Humans set goals + quality; agents self-decide
  everywhere they safely can. Escalation is the exception, batched and validated with the user.
- **Minimalist but functional UI, on a shipped design system.** The panel is light; the power is
  in the terminals + agents. The UI now runs on a shipped, fully-themed **design-token system**
  (light + dark themes, WCAG-AA contrast, a reusable primitives kit — S-UXR `[0.9.0]`)
  (visual/UX rules: `docs/design-system.md`).
- **Honest about state.** The app never fakes session/agent status.
- **Additive, constructor-style growth.** The system extends cube-by-cube; schema and
  architecture grow by addition, never rebuild.

---

## 2. Global locked decisions (apply to every slice)

| Decision | Choice |
|---|---|
| Desktop framework | **Tauri 2** (Rust core + web UI) |
| Distribution | **Universal macOS binary** (arm64 + x86_64), signed + notarized |
| Frontend | **React 19 + Vite + TypeScript**, Zustand for state |
| Strategist role ("SEO" in the brief) | **CEO / strategy orchestrator** — reads goals + graph (+ future metrics), decides what to build |
| Meta-agent execution | **App-native agents** (CEO, PM, engineering specialists) run *inside* the app brain |
| LLM provider for meta-agents | **OpenRouter-first** provider abstraction (also OpenAI / GLM / others behind one interface) |
| Custom agents | User can **add extra app-native agents** at any stage |
| External coding agents | Run as **terminal workers** (claude-code, hermes, opencode, kilo, …) driven by the app brain |
| Terminal control surface | The programmatic terminal API built in S1 **is** the surface the agent brain drives in S6 |
| Methodology | **TDD + DDD**; Superpowers planning cycle (brainstorm → spec → plan → subagent-driven dev) |
| Unattended execution + app-domain store host | **A second launchd-managed daemon `bpa-orchd`** (mirror of the `bpa-sessiond` pattern) owns the app-domain store, the scheduler, the workflow-engine runtime, and the agent runtime; the GUI is its client. `bpa-sessiond`'s charter is unchanged (terminal domain only). See ADR-HOST below. |

**Locks added 2026-07-06 (vision v2–v4):**

- **Multi-project from day one:** every S2+ data model, store, and event stream is multi-project;
  UI panels are per-project VIEWS over multi-project data.
- **Additive-only schema evolution:** entities grow by adding tables/columns; migrations are
  append-only by policy — never a rebuild (vision-v4 §7, «a constructor: brick by brick»).

### ADR-HOST — the two-daemon topology (locked 2026-07-06)

**Problem.** Vision v2's recurring capabilities (the 24h prod-error self-heal loop, metrics
watching, scheduled research) must fire with the GUI closed, survive restarts, and never silently
stop. But app-native agents were slated to run in the core (GUI) process, which dies on window
close; and the Data-layer charter deliberately bars `bpa-sessiond` — the only always-on process —
from becoming the app's general engine or database. As architected, unattended execution was
impossible (audit V2).

**Decision.** A second headless, launchd-supervised daemon — **`bpa-orchd`** — owns: the
app-domain store (all charter entities), the scheduler + event triggers (SW2), the workflow-engine
runtime (SW1), and the agent runtime (S6b+). The GUI attaches to it as a client, exactly as it
attaches to `bpa-sessiond`. `bpa-orchd` drives terminals through the same Hop-B socket protocol as
any client (Pv2 multi-subscriber attach lets orchd and the GUI co-watch one session).

**Rejected alternatives.** (b) Widening `bpa-sessiond`'s charter with an orchestration domain —
riskiest option: couples the hardened, stable terminal daemon to the fastest-moving domain logic.
(c) Honest v0 degradation («schedules fire only while the app runs», catch-up on launch) — fails
the attention-tax mission; kept only as the documented interim behavior until `bpa-orchd` ships.

**Consequences.** orchd reuses the proven patterns verbatim: launchd LaunchAgent lifecycle +
runbook, fail-closed forward-only migrations, the D4/Pv2 drain-and-consent upgrade choreography.
Open sub-question routed to the S-EXT/security spec: Keychain access for unattended runs while
the screen is locked (BL row added this cycle).

### Session survival truth table (canonical — slices reference this, must not drift)

The terminal engine (S1) keeps sessions alive in a detached daemon. What actually survives:

| Event | Sessions |
|---|---|
| GUI close / crash / restart | live shells **keep running** (daemon-owned); reattach + replay |
| Daemon restart / upgrade / crash | live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | die (per-user LaunchAgent torn down); cross-logout survival is out of scope (needs a root LaunchDaemon) |
| Workflow / agent runs | hosted by `bpa-orchd` (launchd-supervised, survives GUI close); a run's step state persists in the app-domain store and resumes/catches-up on orchd restart (catch-up policy per SW2) |
| **`bpa-orchd` restart / upgrade (SHIPPED S3, `[0.4.0]`)** | domain data (projects/goals/ideas/insights/tasks/rules) **fully survives** — it's all SQLite (`orchd.db`), the same durable-store guarantee sessiond's records/scrollback half has. Since S-IDEA (`[0.7.0]`) orchd carries exactly ONE piece of live runtime state: an **in-flight research run** (the async `research::start_run` driver, a detached `tokio::spawn` not covered by the shutdown drain) — a run left `pending`/`running` when the daemon dies is lost and **boot-reconciled to `failed{interrupted}`** on restart (D11; see the S-IDEA row below). Scheduler/workflow/agent runtimes remain roadmap (SW1/SW2/S6b+). Otherwise a restart or upgrade is a non-event for the owner beyond a brief `orchd://down` banner while it reconnects. |

This is stated honestly in-app; the product never claims sessions "survive anything."
(Amended 2026-07-04, A12: the earlier "daemon restart → survive" row overstated — live PTYs die
with the daemon; only records + scrollback survive.)

---

## Protocol evolution & upgrade policy (added 2026-07-04 — A3/A14; owner decisions D3/D4;
amended 2026-07-07 — Pv2 shipped, `[0.2.0]`)

The Hop-B wire (core ⇄ daemon) outlives app updates by design, so its evolution is policy, not
improvisation:

- **Wire discipline:** enum variant order is FROZEN (append-only); new requests/pushes are
  appended; fields are added additively. Every protocol change ships a cross-version decode test.
  Still true post-Pv2 — see the Pv2.1 reserved-batch amendment in
  `docs/superpowers/specs/2026-07-06-protocol-v2-design.md` ("Vision v2–v4 amendments"): future
  request variants (command+argv spawn, typed exit-status wait, `ReadOutput{since_seq}`, a
  rendered-text snapshot) are named and order-reserved now, implemented later, without another
  wire break.
- **Version negotiation (SHIPPED, Pv2 §4):** a codec-agnostic preamble (magic `BPAA`, 5 s bound,
  256-byte build-string cap) carries the client's supported `[min,max]` range; the daemon answers
  `Accepted{chosen}` (highest common version) or `Incompatible{min,max}` — never a silent misparse.
  Now `[3,3]` is exact-match (bumped from `[2,2]` in S2, `[0.3.0]`: multi-root `Workspace.roots` +
  the new `AddWorkspaceRoot`/`RemoveWorkspaceRoot`/`Push::WorkspaceUpdated` verbs are not
  v2-decodable, a planned wire break exactly like v1→v2); the negotiation machinery is built for
  `[min,max]`-style ranges in future cycles. *(Historical: this row previously described
  PROTO_VERSION=1 exact-match with negotiation "landing with v2" — that is now the current state,
  not a future one.)*
- **Daemon upgrade choreography (owner decision D4; SHIPPED, Pv2 §6):** when the core detects an
  incompatible daemon (typed, fatal `IncompatibleDaemon`, never auto-retried), the GUI shows a
  consent dialog — *"Update the background service — N live sessions will terminate. Their records and scrollback
  are preserved and will reappear as inactive"* (N = last known live-session count) — the user
  confirms or cancels. On confirm: best-effort `DaemonShutdown{drain:true}` (flushes scrollback +
  `command_events`, real semantics now — no longer a no-op) → `launchctl kickstart -k` → **the core
  process itself calls `app.restart()`** (a full app relaunch, not an in-place socket reconnect —
  simpler and avoids reasoning about mid-flight state across a version jump); on relaunch the core
  reconnects, negotiates v3, and rehydrated-inactive sessions appear. Kickstart failure surfaces an
  honest banner, never a fake "connected" state. *(Historical: this row previously said SIGTERM +
  `DaemonShutdown` no-op-Ack + in-place reconnect — the app.restart() step and the real drain were
  added during Pv2 implementation, not part of the original plan.)*
- **Release channel:** manual notarized DMG for v0.x; signed + notarized **universal binaries are
  now published on GitHub Releases**; Tauri auto-updater is a named roadmap item
  (BL-19, `docs/backlog.md`).
- **Workspace evolution (A14, SHIPPED 2026-07-09 — S2, `[0.3.0]`):** multi-root workspaces shipped
  as an additive `Workspace.roots: Vec<String>` field alongside the existing `root_path` (compat
  mirror, always `== roots[0]`) — daemon schema v3 adds a `workspace_root(workspace_id, ord, path)`
  table; `AddWorkspaceRoot`/`RemoveWorkspaceRoot` wire requests; `Push::WorkspaceUpdated` →
  `workspace://updated`. *(Historical: this row originally sketched `workspace_roots:
  Vec<PathBuf>` — the shipped wire truth is `roots: Vec<String>` on the `Workspace` struct itself,
  not a separately-prefixed field; `PathBuf` has no portable TS mapping and the `workspace_`
  prefix was redundant on a field already living on `Workspace`. See the S2 design spec §3.1
  naming note, `docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md`.)*

---

## 3. Subsystem decomposition & dependency order

Build spine-first. Each row is an independent spec/plan/build unit.

| # | Subsystem | Depends on | Notes · product DoD · north-star metric |
|---|---|---|---|
| **S0** | App shell + foundation (window, theme, settings) | — | Skeleton everything plugs into. *(Amended: durable storage is DAEMON-owned SQLite, built in S1 — see Data-layer charter.)* **DONE.** |
| **S1** | **Terminal engine** — real PTYs, multi-terminal, lifecycle states, survive-restart, reattach | S0 | Highest technical risk; heart of the product. **DONE** (merged @ 285cb2e). |
| **Pv2** | **Protocol v2** — codec migration (tagged-enum-safe), version-range negotiation, wire-level **multi-subscriber attach** (D5), real `DaemonShutdown{drain}`, `bpa.db` schema-migration policy + `command_events` | S1 | One planned wire break before the protocol grows. DoD: old GUI vs new daemon shows the remediation dialog (never misparses); two subscribers stream one session; cross-version decode tests green. Metric: zero silent protocol failures. **SHIPPED/DONE this branch (`[0.2.0]`).** Two execution deltas vs. the DoD as originally scoped: (1) the upgrade choreography is consent dialog → best-effort drain → `launchctl kickstart -k` → `app.restart()` (a full app relaunch, not an in-place socket reconnect — simpler and more honest about the codec/version jump); (2) cold-rehydrate + attach-inactive shipped as part of this cycle (Task 12r) rather than later — the daemon-restart e2e (closes BL-7) forced the honest "scrollback reappears as an inactive session" path to actually exist, not just be documented. |
| **S2** | Workspace + file explorer (multi-root repos, open/create files & folders, live file-watch) | S0 | React to files agents create; additive `roots: Vec<String>`. DoD: create/open a multi-repo workspace ≤3 clicks; file tree reflects an external `touch` <1 s; explorer stays responsive at 10k files. Metric: time-to-first-terminal in a fresh workspace. **SHIPPED/DONE this branch (`[0.3.0]`).** Deltas vs. the DoD as originally scoped: (1) attention-first Home (originally SH's job) was pulled forward into this cycle — the owner decision (spec D6) judged the daily "where do I need to look" loop couldn't wait for the SH capstone, so `HomeView`'s amber/running/exited queue + one-click «Go» jump ships now, SH inherits it rather than building it fresh; (2) the OSC-133 command strip (spec §6.3) is the first real UI consumer of `command_events` (persisted since Pv2 but unconsumed until now — closes the "no UI" note on BL-31); (3) file I/O + live watch live in the Tauri core, not the daemon (owner decision D4, "Approach A") — `bpa-sessiond` keeps owning the Workspace *data model* only, `fs_explorer`/`fs_watcher` are core-local (GUI-lifetime) modules guarded by the shared `bpa_paths::validate_path_within`, never a Hop-B request. |
| **S3** | **Projects + Goal hierarchy + Ideas + Tasks/Subtasks + RuleSet** data model | S0, S2 (left-rail/Home build on S2's shipped UI), ADR-HOST (orchd store) | The app-domain foundation. Adds: Goal hierarchy (1 strategic + N additional, owner-editable); Idea (nullable project_id — «spawn project from idea»); Insight; unified Task/Subtask (kanban is a VIEW); RuleSet (global + per-project). DoD: goals + ideas + tasks CRUD survive restart; Project⇄Workspace enforced; export/import round-trips. Metric: ideas reaching «specced». **SHIPPED/DONE this branch (`[0.4.0]`).** Deltas vs. the DoD/decisions as originally scoped (S3 spec `2026-07-13-s3-orchd-domain-foundation-design.md` D1–D12): (1) **D2 — daemon-core extraction first:** shared `bpa-daemon-core` (dirs/singleton/logging/migrate/handshake/broadcast) factored out of `bpa-sessiond` FIRST, `bpa-sessiond` re-seated on it byte-identically, THEN `bpa-orchd` built on the same foundation — final architecture immediately, no interim duplication; (2) **D3 — pulled-forward UI:** ⌘K idea quick-capture (originally S-IDEA) and a Home goals panel (originally SH) both ship now, ahead of their capstone slices — the same "can't wait for the daily loop" judgment call S2 made for attention-first Home; NOT pulled forward: research pipeline (S-EXT), kanban (S5), fit-test agent (S-IDEA), the spawn-project-from-idea UI flow (S-IDEA — only its data enabler, `Idea.project_id: Option`, ships); (3) **D4 — files-as-truth RuleSet + narrow file exception:** RuleSet markdown is the source of truth (DB stores `md_path`+`md_hash` only, external edits/deletions surface honestly) — this is a deliberate narrow exception to "orchd gets its own file API in S9" (`docs/architecture.md` amended): orchd touches exactly ONE file family (rules md), not a general file API, which remains S9; (4) **D5 — full goal tree, a deliberate superset:** the charter's "1 strategic + N additional" (two-level) reading shipped instead as a FULL tree — exactly one `strategic` root per project, `additional` subgoals at arbitrary depth via `parent_id` — richer than the DoD strictly required, recorded here as the delta. Q13 (RuleSet markdown + typed policy layer) resolved: BOTH layers ship — markdown (agent-read) is the source of truth (D4), the typed policy layer (spend caps, approval classes, path allowlists) is stored/validated in S3 but gate-ENFORCEMENT is S6c, unchanged from the open-decision default. |
| **S4** | Knowledge graph (per-project + cross-project links, viz) | S3 | `@xyflow/react`; storage + UUID node identity + **agent retrieval API** are S4-spec decisions. **Hard-blocks S6 (owner decision D6) — now unblocked.** DoD: cross-project link survives both projects' restarts; retrieval API returns a goal's subgraph <100 ms; graph editable in UI. Metric: graph nodes retrieved per CEO decision. **SHIPPED/DONE this branch (`[0.5.0]`).** `orchd.db` schema v2 (additive) adds `graph_node`/`graph_edge`; `entityRef` nodes are soft-refs to goal/idea/insight/task rows (D3, no FK — a deleted domain entity leaves an orphan node, UI renders «source deleted»); a strategic-goal `entityRef` node is auto-seeded per project (D6). The workspace-wide retrieval API (D5) — `GraphListProject`/`GraphNeighborhood`/`GraphSearch`, NOT project-scoped — is the concrete S6-agent contract this row promised: a depth-3 neighborhood rooted at a project's strategic goal on a synthetic 500-node/1000-edge graph measures ~51 ms (DoD <100 ms). A cross-project edge (D4, plain edge between nodes of different projects, `ON DELETE CASCADE` within the one store) survives BOTH projects' daemon restarts (e2e phase 5). UI ships as a 7th `ProjectPanel` tab «Graph» (`@xyflow/react` v12, D8/D9); one deliberate scope line honestly drawn: clicking a cross-project ghost node navigates to its project, but clicking a LOCAL `entityRef` node is a no-op for now (no deep-link seam yet from the graph tab into a specific goal/idea/insight/task row in another tab — tracked as follow-up, not silently dropped). Deferred by design (D12), not part of this DoD: any agent runtime that USES the retrieval API (that's S6 itself), auto-population of the graph beyond the D6 strategic seed, and embeddings/semantic retrieval (`docs/backlog.md`). |
| **S-EXT** | **Extensions: MCP client + connectors + skills/plugins** (the Claude Code format) — MCP server registry (add/enable/disable, global + per-project), auth (API key / OAuth 2.1, Keychain), stdio + Streamable HTTP transports, tool discovery + list-changed notifications, typed invoke with retries/timeouts/honest degradation, per-call cost/latency capture, connector accounts (e.g. social networks), skills, management UI | S3 (registry storage), BL-20 Keychain pattern. NOT behind Pv2 or S6a — MCP is orchd/core-outbound JSON-RPC, never Hop-B | Builds on: §16 trust layer + BL-20 Keychain generalize verbatim (V34). DoD: prowl.chat connected; tools listed; one research tool invoked; result persisted as a durable artifact. Metric: MCP tools invocable from workflow steps. Open decision — default: MCP v1 surface = tools + auth only; sampling DISABLED by default; resources/prompts → backlog (Q6). Open decision — default: skills adopt the Claude Code SKILL.md format for portability (Q14). **SHIPPED/DONE this branch (`[0.6.0]`).** The app's first outbound-egress + Keychain surface, entirely in `bpa-orchd` (two new crates: `bpa-secrets` for Keychain, `bpa-mcp` wrapping the official `rmcp` SDK). Shipped: `orchd.db` schema v3 (MCP server/tool registry, connector accounts, invocations, durable untrusted artifacts, skills, and the trust layer's consent/policy/audit tables); BOTH transports (Streamable HTTP for remote servers, stdio for local processes behind a dedicated `stdio_exec` consent gate); per-tool allowlist; typed `tools/call` with transport-only retry + timeout + honest degradation; OAuth 2.1 (PKCE) AND api-key connector accounts, tokens always in Keychain; one reference `generic-rest` connector adapter, `ConnectorInvoke` sharing the identical trust + durable-artifact path as an MCP tool call; the full BL-22 trust layer (connect/stdio-exec consent with fingerprint re-prompt, per-tool allowlist, spend/rate policy caps — most-specific-scope-wins, spend binds only when a server reports cost — untrusted-tagging, append-only audit log); a shared `DYLD_*`/`LD_*` env denylist closing **BL-1** for BOTH orchd's stdio spawn AND sessiond's `env_overrides`; a SKILL.md skills registry (files-as-truth, **plumbing only** — no runtime consumer until S6b, stated honestly in the UI); the «Extensions» management UI (Servers/Tools/Connectors/Log/Artifacts/Skills). DoD proof: the e2e harness proves the full connect→list→invoke→durable-artifact-survives-restart mechanism against a LOCAL stub MCP server (phase 6) and an analogous local stub for a connector invoke (phase 7, with a Keychain-availability probe-and-skip for headless CI) — wiring the *real* prowl.chat account is a documented, non-blocking Human step (owner-supplied credential). Deferred by design, filed in `docs/backlog.md`: MCP sampling and resources/prompts (Q6, as scoped); a named social adapter (X/LinkedIn) beyond the generic-rest reference; active tool-result prompt-injection mediation at the agent boundary (the tag is set now, S6b consumes it); a bulk MCP-server import; a LIVE `tools/list_changed` subscription (Phase-1 has no session held open between calls — the cache instead refreshes wholesale on every `McpConnect`); a stdio child's stderr is inherited to orchd's log unredacted; BL-27 (Keychain-while-locked, for a future *unattended* run) re-targeted to S6b/SW2. **S-EXT unblocks S-IDEA** (its research trigger depends on this MCP substrate) and feeds the tool-provider registry S6b's agent runtime will consume. |
| **S-IDEA** | **Ideas + research pipeline** — quick-capture UI (⌘K), idea inbox + lifecycle chips, «add to project» / «spawn project from idea» (creates Project+Workspace scaffolding via S2/S3), research trigger (prowl via S-EXT) with inline spend approval, streaming research pane, ResearchArtifact viewer, **insight fit-test vs goals/metrics**, task formation + decomposition handoff. **Ships BEFORE the agent org** — the first user-visible v2 value | S3, S-EXT, S4 | DoD: idea → prowl research → evaluated insight → task lands in the backlog, end-to-end, without the S6 agent org. Metric: ideas reaching «specced» per week. Open decision — default: one prowl session_id per research-run (Q5). Open decision — default: prowl down ⇒ inline «form task without research» with an honest-degradation note on the artifact chain (Q8). Open decision — default: insight fit-verdict computed by an agent WITH one-click owner override; no auto-accept (auto-archive of clear non-fits allowed, reasoning kept) (Q10). **SHIPPED/DONE this branch (`[0.7.0]`).** `orchd.db` schema v4 (additive, `SCHEMA_VERSION` 3→4) adds exactly ONE net-new table, `research_run` — the "ResearchArtifact" named in the DoD above is NOT a separate entity/blob store, it's the REUSED S-EXT `mcp_artifact` produced by the run's tool call (D2, no blob duplication; the entity-map row below is corrected to say so). The async run driver (`research::start_run`, orchd's FIRST long-lived `tokio::spawn`) is 3-phase-locked (D3, never holds the DB mutex across the network await) and boot-reconciles any run still `pending`/`running` at daemon start to `failed{interrupted}` (D11 — closes the crash/restart risk a detached background task otherwise carries, since it isn't tracked by the shutdown drain); the same pass also bounds the shipped MCP `connect`/`initialize` handshake by `server.timeout_ms` (D12 — a hang-forever fix in the S-EXT invoke path, benefits every MCP call, not just research). Three wire verbs (`ResearchStartRun`/`ResearchListRuns`/`ResearchGetRun`) + `ResearchRunsChanged`; the frontend idea→research→insight→task flow (`ResearchRunDialog`/`ResearchPane`/`FormInsightDialog`/`SpawnProjectFromIdea`) reuses the S-EXT artifact viewer + untrusted-data banner and the S4 graph-neighborhood component — closing BL-56 (the spawn-project-from-idea UI flow S3 deferred). Deltas vs. the DoD/decisions as originally scoped: (1) **Q10 override (D4) — owner-driven fit-verdict, NOT agent-computed:** S6a (the native LLM provider layer) is not built, and the DoD itself requires the loop to work "WITHOUT the S6 agent org" (S6a is a member of it) — v1's `fit_verdict`/`fit_reasoning` are owner-set beside a fit-context panel (the project's goals+`metric_refs` + a `GraphNeighborhood` read); LLM auto-scoring is filed to backlog for S6a, not silently dropped; (2) **Q5 v1-override (D13) — no BPA-generated `session_id`:** each `research_run` is one run-scoped tool call, so run-level isolation already holds without one; a prowl `session_id` is owner-supplied via `args_json` like any other tool arg (v1 does not hardcode a prowl-specific schema, per the spec's own non-goals) — a prowl-aware convenience adapter that auto-seeds one is backlog; (3) **the research pane is NOT token-streaming:** MCP `tools/call` is request/response in the shipped connect-per-call model, so v1 shows run status (pending→running→done/failed), not streamed tokens — an honest scope line, not a partial build (a streaming pane needs a persistent-session architecture, aligning with the S-EXT `list_changed` backlog item BL-70); (4) task decomposition is owner-driven (manual subtasks via `CreateTask{parent_id}`) — automated decomposition is S6b's job. DoD proof: `npm run e2e:orchd` phase 8 (idea→research→insight→task survives a daemon restart) and phase 9 (a run interrupted mid-flight reconciles to `failed{interrupted}` on restart, proving D11). |
| **S-POLISH** | **Reliability + honesty + English-only + Tier-2 feature completeness** — a consolidation slice, not a new subsystem: backend reliability/observability (connect + OAuth timeouts, import-after-commit, storage-degradation on the wire, per-verb tracing), a full English-only sweep with a CI no-Cyrillic gate, frontend reliability across every mutating/read surface (submit-guard, toast queue, reconnect rehydration, honest empty/loading/failed states), and four Tier-2 feature gaps closed (project un-archive, `metric_refs` editor, graph edge-kind/node editor, config-backed OAuth registry) | S-IDEA (+ every prior slice it hardens) | DoD: no hang-forever external call; the app never lies about durability or session/agent state; every UI string + living doc is English (gate-enforced); no one-way archive trap; the graph is fully editable; the OAuth registry is reachable. Objectives O-1..O-7 (`docs/superpowers/specs/2026-07-16-s-polish-program-design.md`). **SHIPPED/DONE this branch (`[0.8.0]`).** No wire version bump — `bpa-orchd` stays `[1,1]`, and all four net-new verbs (`GetStorageStatus`, `UnarchiveProject`, `GraphUpdateEdge`, `ConnectorListProviders`) are appended at the enum TAIL (append-only); no schema migration lands. **O-1** (term mapping: hypothesis = Idea/Insight, feature = DomainTask) confirmed, catalog stands, no re-cut. **O-2** English everywhere + the standing rule + `scripts/check-english.sh` CI gate (final-suite stage 1). **O-3** project archive + a NEW `UnarchiveProject` verb + UI controls (closes BL-53, the one-way-trap gap). **O-4** the `metric_refs` owner chip-editor on goals (pure frontend, reuses the shipped `UpdateGoal` verb). **O-5** the config-backed OAuth provider registry (`<app-support>/oauth_providers.json`, honest missing/malformed degradation, names-only on the wire via `ConnectorListProviders`) — which activates the BL-91 OAuth-exchange timeout on a now-reachable path. **O-6** per-verb structured completion tracing (`verb`/`outcome`/`error_code`/`elapsed_ms`, one wrapper per dispatch layer, no secrets). **O-7** the graph editor (node title/body form + inline rename + the new `GraphUpdateEdge` edge-kind verb). Also closes the P1–P3 reliability backlog: **BL-89** (McpConnect timeout), **BL-90** (import file-before-commit), **BL-91** (OAuth-exchange timeout), **BL-92** (reconnect rehydration + research self-poll), **BL-93** (silent no-op / zombie-tab triad), **BL-94** (storage-degradation banner), **BL-95** (double-submit guard + partial-failure resume), **BL-96** (reopenable orchd-upgrade banner), **BL-97** (toast FIFO queue). Re-measured totals: Rust 1023 → **1062 tests**, TypeScript 772 → **870 tests** (47 → 51 files). DoD proof: the same 10-stage `scripts/final-suite.sh` (now with the English gate as stage 1) stays green. |
| **S-UXR** | **Frontend design-system redesign + UX-scenario base** — a design-token system (light + dark themes, WCAG-AA contrast), a reusable primitives kit, and a 181-scenario UX base every panel is rebuilt against | S-POLISH (+ every prior frontend surface it restyles) | A frontend consolidation slice, not a new backend subsystem: no wire/schema change. DoD: every panel renders on the shared design tokens; light + dark themes both pass WCAG-AA contrast; the primitives kit is the single source of UI truth. **SHIPPED/DONE this branch (`[0.9.0]`).** Spec: `2026-07-18-s-uxr-redesign-scenario-base-design.md`. |
| **S-DIAG** | **Reconstructable diagnostics error log + ErrorBoundary** — a structured, replayable frontend diagnostics/error log plus a React `ErrorBoundary` so a UI fault degrades honestly instead of blanking the app | S-UXR | DoD: an unhandled UI error is caught, logged reconstructably, and shown as an honest failed state — never a white screen. **SHIPPED/DONE this branch (`[0.9.1]`).** Spec: shared with S-UXR. |
| **S-DESIGN** | **WCAG-AA contrast guard** — a contrast pass + guard across the token themes so both light and dark stay AA-compliant | S-UXR | DoD: light + dark token themes pass WCAG-AA contrast, guarded against regression. **SHIPPED/DONE this branch (`[0.9.1]`).** Spec: shared with S-UXR. |
| **S5** | Kanban **as a VIEW over the Task entity** (backlog / todo / waiting / progress / testing / done) | S3 | `@dnd-kit`; minimalist. A card projects a Task/Subtask row; column derives from task/workflow-run status; agent- and engine-driven moves render live. DoD: drag persists; agent-driven card moves render live; column WIP counts. Metric: cards moved by agents vs by hand. Open decision — default: per-project stack rank (plan+bug mixed) + panel-level cross-project rank; owner ranks HARD-override agent ranks (suggestions with visible reasoning) (Q9). |
| **S6a** | **LLM provider layer + tool-calling** — provider trait; OpenRouter/OpenAI/GLM adapters; routing + fallback; streaming; retries; **tool-use/function-calling in the trait** (tool defs in requests, streamed tool_call deltas, cross-provider normalization); **per-call cost/token/latency capture from the first call** | S4, Pv2 | Keys in Keychain (BL-20); egress core/orchd-only. LLM model-providers (this) and MCP tool-providers (S-EXT) are two DIFFERENT abstractions. DoD: same prompt runs on 2 providers via config only; a tool-call round-trip works on 2 providers; provider outage fails over with an honest event; every call logged with cost. Metric: $/task visible per run. |
| **SW1** | **Workflow engine core (workflow-as-data)** — versioned WorkflowDefinition (format_version, append-only evolution; runs pin their version), ordered/DAG steps, step kinds: agent-turn \| terminal-command \| mcp-tool \| data-fetch \| data-load \| data-process \| insight-extract \| human-approval; step executor; manual trigger; **run-observability contract**: every StepRun records {input ref, request, response ref, processing actions, timings, cost} | S3, Pv2, S6a, ADR-HOST | Builds on: Hop-B + command_events + multi-subscriber attach = the terminal-step substrate — reuse, don't reinvent (V32). DoD: a 3-step definition (mcp-tool → data-process → human-approval) authors, runs, and its run history shows per-step I/O drill-down. Metric: % of orchestration flowing through definitions (target: 100% by S6b). |
| **S6b** | **Agent runtime + ONE role end-to-end, running VIA the engine** — the CEO→PM→eng loop is the DEFAULT WorkflowDefinition executed by SW1, not bespoke control flow; PM gap-analysis on a real repo; agents operate managed projects by the meta-process (§6) | SW1 | Hosted by `bpa-orchd` (survival row resolves). DoD: PM produces a spec+task list from goals+graph on a live repo via the default definition; run survives GUI close and orchd restart; audit log complete. Metric: tasks completed without human touch. |
| **S6c** | **Approval inbox — the ONE generic gate** — intake: agent escalations AND workflow human-approval steps AND policy gates (spend, destructive, deploy, spec approve); single queue across ALL projects; batch-approve | S6b (UI), SW1 (gate semantics) | Builds on: this inbox IS the home screen's hot-questions queue (V33) — bind, don't fork. DoD: gated action blocks until approval; batch-approve works; every agent command in the audit log. Metric: escalations per completed task (target ↓). Open decision — default: hard gates = task-breakdown approve + spec approve + deploy; deploy auto-passes LOW-RISK diffs = no DB migrations AND diff < 300 LOC AND paths within the project allowlist — deterministic rules, no LLM classifier in v1; per-project policy may tighten/loosen (Q2). Open decision — default: away-from-Mac = macOS notification only in v0.x; remote answering = backlog (Q3). |
| **SW2** | **Scheduler + event triggers + domain event bus** — cron \| event \| manual triggers on definitions; persisted schedules with next_fire_at rehydration; missed-run catch-up policy; overlap control; approval-suspend (a gated run parks without blocking the scheduler) | SW1, ADR-HOST (host), S6c (suspend/alert routing) | DoD: a 24h schedule fires with the GUI closed; a missed window catches up per policy; two overlapping fires resolve per policy; run pauses on a gate and resumes on approval. Metric: scheduled-run reliability (fired/expected). |
| **SW3** | **Visual workflow editor** — canvas definition editor (add/reorder/edit steps), three-tab tool-binding picker (Agents / Terminals / MCP tools), trigger + gate/policy editors, validation, «run now», run-history view with per-step I/O | SW1, SW2, S-EXT | DoD: the owner composes the 24h self-heal chain end-to-end in the editor, enables it, edits it later (adds a step) without breaking pinned runs. Metric: definitions authored/edited by the owner (not shipped defaults). |
| **S6d** | **External worker adapters** (claude-code, hermes, opencode, kilo) — per-CLI liveness/stuck detection (raw-mode TUIs are invisible to `waiting_for_input` — adapters own this) as step workers | S6b | DoD: adapter detects worker-waiting within 5 s across all 4 CLIs; wedged worker auto-escalates. Metric: mean time-to-unstick. |
| **S6e** | **Custom-agent authoring + configurable agent org** (roles, models, policies, prompts for the CEO and every org agent) | S6c | 'Tools' = Tool Registry ids (S-EXT + terminals + agents); custom agents/chains run under the same trust layer + audit. DoD: owner creates a custom role in UI; it runs gated + audited. Metric: custom agents in weekly use. |
| **S9a** | **Project telemetry ingestion + study folder** — per-project LogSourceConfig registry (error-tracker APIs, hosting log pulls, file tails, MCP-backed connectors; credentials in Keychain), pull/tail into ErrorGroups (dedup), StudyItem triage queue | S3 (project entity), ADR-HOST | DoD: a real project's prod errors land as deduped ErrorGroups; new groups enqueue StudyItems; retention/redaction per charter telemetry class. Metric: time from prod error to StudyItem. Open decision — default: enabling the self-heal workflow REQUIRES a tested rollback recipe in the deploy config — refused with an actionable message otherwise; run-budget breach ⇒ pause as gated escalation (Q7). |
| **S9b** | **Delivery/deploy step + verification** — per-project deploy config (command/target), execution via terminal workers (command_events = the run record; not a CD system), deploy gate (S6c), post-deploy verify step (error-rate/smoke re-check; a run may not mark itself successful without it) | SW1, SW2, S9a, S6c | DoD: the self-heal definition deploys a fix and verification confirms the error group went quiet — or the run honestly reports otherwise. Metric: verified fixes shipped per week. |
| **S7** | Stats / observability — time worked, task counts, stages, errors, logs, LLM traces, spend/budgets, evals, **workflow-run health** (last success, consecutive failures, missed runs, per-run cost rollups, dead-loop alerts) | S6b, SW2 (run history) | `recharts`. DoD: per-project spend dashboard; per-agent success/error rates; budget alert fires; scheduled-run health visible. Metric: % runs within budget. |
| **S8** | **Metrics ingestion + metrics→sprint** — per-project external metrics/analytics via the S9a connector substrate; MetricPoint timeseries (append-only, retention/downsample); **MetricDefinition: owner-declared MAIN metrics, mutable**; the metrics-watch→sprint-planning workflow (SW1 definition); insight evaluation reads metrics | S3 (data), SW1/SW2, S9a (connectors); S7 only for dashboards | Builds on: S6a/MCP call records are the first MetricPoint sources (V35). DoD: a project declares main metrics; a metrics-watch run drafts a sprint plan citing them. Metric: CEO/sprint decisions citing declared metrics. Open decision — default: metrics are goal-attached (each goal names the metric(s) it moves); project settings is the editor (Q12). Open decision — default: experiments = Tasks tagged `experiment` (hypothesis/metric/verdict in body); first-class entity deferred (Q11). |
| **SH** | **Mission-control home (capstone)** — six lanes per project (goals, plan tasks, bug-fix tasks, prioritization, delivery/workflow state, monitoring) + live agent feed + «what moved» deltas + Insights lane + idea quick-capture + hot-questions inbox + agent-org settings + vector steering | S6c, S7, SW1 run state, S-IDEA, S9a/S9b | DoD: north-star metric (a) holds — open the app → full 5-6-project context < 30 s; every element live, no manual refresh. Metric: time-to-context (measured). |

```
S0 ─┬─ S1 ─ Pv2 ─────────────────────────────┐
    ├─ S2                                     │
    └─ S3 ─┬─ S4 ────────┬────────────────────┤
           ├─ S5         │                    │
           ├─ S-EXT ─────┴─ S-IDEA            │   (S-IDEA = first v2 value,
           │                                  │    BEFORE the agent org)
           └──────────────────────────────────┤
                                              ▼
                    S6a ─ SW1 ─┬─ S6b ─┬─ S6c ─ S6e
                               │       ├─ S6d
                               │       └─ S7 ◄─ SW2 (run history)
                    SW1 ─ SW2 ─┴─ SW3 ◄─ S-EXT   S9a ─ S9b
                    (deps NOT drawn as arrows, from the table:
                     SW2 = SW1+ADR-HOST+S6c; SW3 = SW1+SW2+S-EXT;
                     S7 = S6b+SW2; S9a = S3+ADR-HOST; S9b = SW1+SW2+S9a+S6c;
                     S8 = S3+SW1/SW2+S9a; SH = capstone over
                     S6c + S7 + SW1 run state + S-IDEA + S9a/S9b)
```

**Current slice:** S0+S1 **DONE**; docs-truth/CI pass **DONE**; vision-alignment pass **DONE**;
Pv2 (Protocol v2) **DONE** (`[0.2.0]`); S2 (multi-root workspaces + file explorer +
attention-first Home) **DONE** (`[0.3.0]`); S3 (`bpa-orchd` + app-domain foundation: projects/
goals/ideas/insights/tasks/rulesets) **DONE** (`[0.4.0]`); S4 (knowledge graph + workspace-wide
agent retrieval API) **DONE** (`[0.5.0]`) — S6 unblocked (owner decision D6); S-EXT (MCP client +
connectors + skills, the app's first outbound-egress + Keychain surface) **DONE** (`[0.6.0]`) —
S-IDEA's research trigger unblocked, and S6b's agent runtime gains a ready-to-consume
tool-provider registry; **S-IDEA (ideas + research pipeline: `research_run` schema v4, the async
run driver + boot-reconcile, the idea→research→insight→task loop) DONE (`[0.7.0]`) —
the first user-visible v2 value, shipped WITHOUT the S6 agent org (fit-verdict is owner-driven;
LLM-computed scoring is deferred to S6a, filed to backlog).** S-POLISH (reliability + honesty +
English-only + Tier-2 feature completeness) **DONE** (`[0.8.0]`); S-UXR (UX-scenario base +
design-token/primitives redesign, light+dark theme, WCAG-AA contrast) **DONE** (`[0.9.0]`);
S-DIAG/S-DESIGN (a reconstructable diagnostics error log + ErrorBoundary + WCAG contrast pass)
**DONE** (`[0.9.1]`); BL-101 (boot-race fix: AppState managed synchronously in `setup()`)
**DONE** (`[0.9.2]`). **Shipped-through: `[0.9.2]`.**
Next: S5 (kanban) → S6a (LLM layer) → SW1 → …
Spec: `2026-07-01-builderpro-s0s1-foundation-terminal-design.md` (S0+S1);
`2026-07-08-s2-workspace-explorer-home-design.md` (S2);
`2026-07-13-s3-orchd-domain-foundation-design.md` (S3);
`2026-07-14-s4-knowledge-graph-design.md` (S4);
`2026-07-15-s-ext-mcp-connectors-design.md` (S-EXT);
`2026-07-15-s-idea-research-pipeline-design.md` (S-IDEA);
`2026-07-16-s-polish-program-design.md` (S-POLISH);
`2026-07-18-s-uxr-redesign-scenario-base-design.md` (S-UXR / S-DIAG / S-DESIGN).

---

## 4. How the agent layer will use the terminal engine (forward contract)

*(Rewritten 2026-07-04 — A4/A5/A6/A16; owner decision D5. Amended 2026-07-06, vision v2–v4.)*

**The canonical programmatic agent API is the Hop-B socket protocol** (create / attach /
write-stdin / stream-output / resize / kill / query-state) — not the Tauri command list, which is
merely the GUI's thin wrapper over it. Agent process model: **app-native agents (CEO/PM/eng) run
in `bpa-orchd`** (ADR-HOST, §2) and reach the terminal daemon over the same socket client;
**external worker CLIs run inside PTY sessions** as ordinary children.

**The loop is a definition, not code (vision v2–v4).** The CEO→PM→eng orchestration below ships
as the **DEFAULT built-in WorkflowDefinition executed by the SW1 engine** — editable data, never
compiled control flow. Built-in definition #2 is **goal-driven research/refresh**: per goal, a
recurring research run produces Insights (canonical example: goal «boost traffic and acquisition
channels» → research → N candidate channels → N separately testable items). Step kinds
available to any definition: `agent-turn | terminal-command | mcp-tool | data-fetch | data-load |
data-process | insight-extract | human-approval`. **Run-observability contract:** every StepRun
records {input data ref, outgoing request, received data ref, processing actions, timings, cost}
— the owner can open any run and see, per step, what came in, what was asked, what came back.

The default orchestration definition (unchanged vision):

1. **CEO** picks the next objective from goals + knowledge graph (+ future S8 metrics). *(S4 is a
   hard prerequisite — owner decision D6.)*
2. **PM** (TDD+DDD) does gap analysis (current vs 100%-done), writes specs, decomposes into tasks,
   assigns each to an engineering specialist (ML / data-sci / security / backend / frontend / testing).
3. Each specialist **spawns / drives a terminal worker** (claude-code, etc.) and watches it via the
   worker adapter (below).
4. Unresolvable questions bubble PM → CEO, which batches them and **validates with the human**,
   then feeds answers back to the workers.
5. Kanban (S5) reflects the live state; Stats (S7) records it.

**Contract corrections locked now (so S6 doesn't build on false assumptions):**

- **Co-viewing (owner decision D5, SHIPPED at the wire/daemon level in Pv2 `[0.2.0]`):** human and
  agent watching one session simultaneously is served by **wire-level multi-subscriber attach** (N
  subscribers per session, keyed `(session, conn)`, each getting its own Replay + backpressure) —
  the daemon supports this today. No UI consumer exists yet: the GUI remains a single subscriber
  until S6 wires a second (agent) client. *(Historical: this row previously said "designed and
  implemented in protocol v2" as a forward-looking statement with S1 single-attach standing until
  then — Pv2 has since shipped, so the wire/daemon capability described is now real, not planned.)*
- **`waiting_for_input` honest scope:** it is a **canonical-mode line-input heuristic only**. It
  is structurally blind to raw-mode TUIs — which includes every named worker CLI (claude-code,
  opencode, …). Worker liveness / stuck detection is therefore a **named S6d subsystem** (worker
  adapters, per-CLI strategies), NOT this flag.
- **Planned additive agent capabilities** (requests to add, append-only): `ReadOutput{since_seq}`
  cursor read; rendered-text snapshot (grid as text); command+argv spawn (no shell wrapping);
  typed exit-status wait.

This is why S1's programmatic control is first-class in the S0+S1 spec even though the agents
come later.

---

## Data-layer charter (added 2026-07-04; amended 2026-07-06, vision v2–v4)

- **`bpa-sessiond` owns terminal-domain durable state ONLY** (sessions, scrollback,
  workspaces-as-terminal-roots, `command_events` in `bpa.db`). It must never become the app's
  general database.
- **All app-domain entities live in the `bpa-orchd` store** (ADR-HOST, §2). The concrete engine
  is decided in the S3 spec; the OWNERSHIP boundary is locked here.
- **Project ⇄ Workspace (locked):** *a Project is the planning entity that owns goals / graph /
  kanban and maps to 1..N Workspaces (repo roots).*
- **S4 owns:** graph storage decision, UUID node identity, cross-project link model, and the
  **agent retrieval API** — read AND write, **workspace-wide** (an agent working project A can
  query and read project B's knowledge). S4 **hard-blocks S6** (owner decision D6).

### Entity map (owning slice + store)

| Entity family | Store | Owning slice |
|---|---|---|
| Project | orchd | S3 |
| Workspace (repo roots; bridged to sessiond terminal-roots) | orchd (+ sessiond bridge) | S2/S3 |
| Goal — hierarchy: 1 strategic + N additional, owner-editable (vector steering) | orchd | S3 |
| Idea — lifecycle captured→researching→specced→in-dev→shipped; nullable project_id (spawn-project flow) | orchd | S3 |
| Insight — market-sourced; fit-verdict vs goals/metrics; accepted→backlog \| archived-with-reasoning | orchd | S3 (+S-IDEA) |
| Task / Subtask — one unified model; kanban is a VIEW; sources: idea \| insight \| bug \| plan | orchd | S3 |
| RuleSet — global + per-project rules; markdown layer (agent-read) + typed policy layer (gate-enforced) | orchd | S3 |
| ResearchArtifact — NOT a distinct table: the REUSED S-EXT `mcp_artifact` a research run's tool call produces (no blob duplication, S-IDEA spec D2) | orchd | S-EXT (+S-IDEA reuse) |
| ResearchRun — the S-IDEA net-new row: idea↔MCP-invocation↔artifact provenance + `pending`/`running`/`done`/`failed` status; boot-reconciles any run stuck non-terminal at daemon start to `failed{interrupted}` | orchd | S-IDEA |
| WorkflowDefinition — versioned, format_version, append-only evolution | orchd | SW1 |
| WorkflowRun / StepRun — run history; StepRun records {input ref, request, response ref, processing, timings, cost} | orchd (+ run/step logs) | SW1 |
| Schedule / Trigger — cron \| event \| manual; next_fire_at; catch-up + overlap policy | orchd | SW2 |
| MCP / Connector / Skill registry — servers, transports, enabled tools, accounts; global + per-project | orchd (secrets → Keychain) | S-EXT |
| MetricDefinition (owner-declared MAIN metrics, mutable) + MetricPoint (timeseries) | orchd | S8 |
| ErrorGroup / StudyItem (prod telemetry triage) + Deploy record | orchd | S9a/S9b |

- **Historical telemetry:** worker transcript log + `command_events` land at the daemon schema-v2
  bump (Pv2) — with a workflow-run attribution hook (Pv2 amendment).

### Global storage architecture

One map, four stores — growth-ready, additive-only (§2 lock):

1. **`bpa-sessiond` `bpa.db`** — terminal domain (sessions, scrollback, command_events). Unchanged.
2. **`bpa-orchd` app-domain store** — the 14 entity families above; ships schema v1 with its own
   `user_version`, **fail-closed, forward-only, single-transaction migrations** (mirrors sessiond
   persistence); additive-only by policy.
3. **Run/step logs** — StepRun I/O payloads. Default retention (Q15): full payloads 14 days →
   thinned to metadata (summaries + sizes + hashes); 50 MB per-run cap with honest truncation
   markers.
4. **Artifact blobs** — research artifacts and large payloads, referenced by id from orchd rows.

- **Cross-store references are SOFT:** UUID strings, no FK-integrity assumption across stores;
  a store must keep its own copy of anything it cannot afford to lose to another store's
  retention (e.g. BL-4 scrollback pruning must not destroy workflow-run evidence — orchd keeps
  what it needs).
- **Ingested telemetry is a distinct data class:** prod logs/errors and metrics may carry
  PII/secrets; store home + redaction + retention are decided in the S9a spec; purge-on-delete
  ties into project deletion.

---

## 5. Human-in-the-loop boundary

*(Amended 2026-07-06, vision v2–v4.)*

Autonomy is the default. The sanctioned human steps: setting/adjusting **goals + quality bars**
(vector steering); **authoring and editing workflow definitions, schedules, step tool-bindings,
and rules** (global + per-project); answering the **single inbox** (batched escalations +
workflow gates the agents genuinely can't resolve); **connecting MCP servers / connectors /
accounts**; and providing **credentials/access** (API keys, repo access). Everything else is
agent-decided.

---

## 6. Process

Every slice follows the Superpowers cycle: `brainstorming → spec → writing-plans →
subagent-driven-development`, with contracts locked in the spec, non-overlapping file ownership
for parallel tasks, TDD throughout, and a per-task Definition of Done. External-library contracts
are verified against current docs (Context7 + web) before locking — never from memory.

### The meta-process law (added 2026-07-06, vision v4 §8)

For the platform AND for every project managed inside it:

1. The **end goal is always visible and editable**; editing it triggers re-planning.
2. A **live step-plan to that goal** is always kept, and re-actualized whenever the goal changes.
3. **Architecture and data structures are designed first**; then a minimum is defined and
   extended constructor-style — cube by cube, additive, never a rebuild (§2 lock).

The CEO/PM agents must operate managed projects by this same method — this binds the S6b spec.
