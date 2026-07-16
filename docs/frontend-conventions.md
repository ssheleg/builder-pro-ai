# Frontend platform conventions (S2+ contract)

Written 2026-07-04 (audit A32). Binds every future frontend slice; extends the S0+S1 spec §12.

## Store architecture (Zustand)

- **Slice-per-feature** for S2+: `terminal`, `workspaces`, `kanban`, `graph`, `agents`,
  `settings` — each slice is its own module with its own actions, composed into ONE store.
  Today's single-file store (`src/store/store.ts`) is the `terminal`+`workspaces` seed; split it
  when the next feature slice lands, not before.
- **Locked invariant: PTY bytes NEVER enter React state.** The output firehose goes straight
  into xterm `Terminal` instances owned by `TerminalManager`
  (`src/terminal/terminal-manager.ts`), which lives OUTSIDE Zustand/React. The store holds
  metadata only (`SessionMeta`, ids, flags). This is the single most important data-flow
  decision for scaling to N agent terminals — do not regress it.

## Events

- Naming rule: **`<domain>://<event>`**. The real set today (emitted by
  `src-tauri/src/broker.rs::EV_*`, subscribed in `src/ipc/events.ts`):
  `session://created`, `session://state-changed`, `session://exited`,
  `workspace://created`, `daemon://disconnected`, `daemon://reconnected`.
- New domains follow the same rule (`kanban://…`, `agent://…`); payload types come from
  `src/ipc/types.ts` (ts-rs generated — never hand-edited).
- One **subscription-registry module per slice**: a slice registers all its listeners in one
  place (the pattern `src/ipc/events.ts` + `App.tsx` uses today), returning unlisten handles.
- **Documented gap:** NO `daemon://connected` fires on first connect — the contract is
  hydrate-until-success (bounded-backoff `list_workspaces`/`list_sessions` until they succeed;
  see the doc comment in `src/App.tsx`). Don't invent a connected event; if protocol v2 adds one,
  it replaces this paragraph.

## Attach state

Per-session state machine `detached | attaching | attached` owned by `TerminalManager` —
coalescing, generation guard, reconnect reset semantics. Normative statement: S0+S1 spec §12
«Attach state contract». Panes call `attach()` unconditionally; the manager dedupes. Never put
attach guards in component refs (a pane instance is reused across tab switches — audit A1).

## Design tokens

- Source of truth: `src/theme.ts` (`Theme.colors`: `bg`, `bgElevated`, `border`, `text`,
  `textDim`, `accent`, `statusIdle`, `statusRunning`, `statusExited`, `statusWaiting`).
- Status-color semantics are part of the product language (grey=idle, green=running,
  amber=waiting-for-input, red=exited) — reuse them everywhere state is shown (kanban cards,
  agent dashboards), never re-invent.
- At the next UI slice: extract spacing / typography / radius into the same module and expose all
  tokens as CSS variables; until then inline styles read `theme` directly (current pattern).
- **Color scheme posture: dark-only for v0.x** (explicit decision; light mode is future work).

## Extensions view — `ExtPanel`, consent dialogs, untrusted banners (S-EXT)

- **`view` union widened, not a registry (code-truth):** the top-level `view` string-union in
  `store/store.ts` (`"home" | "workspace" | "project"`) gains `"ext"` — adding a new top-level
  view touches exactly three files, named here so nobody rediscovers them by grep:
  `store/store.ts` (the union + default), `components/WorkspaceSidebar.tsx` (the inline nav
  button — nav buttons live inline there, no separate `LeftRail` component), `App.tsx` (the
  `view === "ext"` branch in the `if`/`else` render chain).
- **`ExtPanel` mirrors `ProjectPanel`'s tab pattern:** one panel component owns tab-selection
  state and renders exactly one child per tab (Servers/Tools/Connectors/Log/Artifacts/
  Skills — `src/components/ext/*.tsx`), same shape as `ProjectPanel`'s Overview/Goals/Ideas/…
  tabs — no new tab-panel pattern invented for this view.
- **Honest degradation, unchanged discipline:** every mutating control across every `ext/*` tab
  (add/enable/disable/connect/set-bearer/invoke/OAuth-begin/api-key-add/delete/set-policy/
  skill-add/skill-delete) is `disabled={orchdDown}` — read-only affordances (lists, the tools
  browser's read view, search) stay live. Tested per-tab: populate the form first, flip
  `orchdDown`, assert `disabled` AND assert the wrapper is never called on a click (userEvent,
  which respects `disabled` — plain `fireEvent` does not).
- **`ConnectDialog`:** every connect attempt from `ServersTab` routes through this one dialog
  (no "already consented" signal exists on the wire `McpServer` entity to gate a
  dialog-vs-direct-connect choice on, and `trustGrantConsent` is idempotent, so always
  confirming is simpler and honest) — `Connect` grants `TrustGrantConsent(id, "connect")`
  THEN calls `mcpConnect`, in that order, since `mcpConnect` is trust-gated and rejects with
  `Error{Consent}` until the grant exists. A failure (network/consent/policy) is shown IN-dialog
  (`role="alert"`) via `describeOrchdError`, not just a toast. **`ServersTab`'s transport picker
  is fixed at `"http"` for now** — a `"stdio"` option is present but disabled ("soon"): the
  backend's distinct `stdio_exec` consent kind (a different fingerprint scheme, binary-hash not
  URL) is fully built (`crates/orchd/src/trust.rs`), but no UI flow can create a stdio server
  yet, so there is no separate stdio-exec consent dialog to document — don't assume `ConnectDialog`
  covers it until a future task wires up the stdio transport picker.
- **Untrusted-result banner:** any surface that renders an MCP tool result or a connector-invoke
  result (`ToolsBrowser`'s invoke-result panel, `ArtifactsTab`'s per-row viewer) shows a fixed
  «unverified data» banner — unconditional for any result at all (every `mcp_artifact` is
  `is_untrusted=1` by construction, S-EXT spec D9), never computed from response content. Treat
  this the same way as the graph canvas's orphan/ghost styling: a static, tested badge driven by
  a boolean flag from the wire, not inferred client-side.
- **Skills tab's plumbing-only banner:** `SkillsTab` renders a fixed, unconditional
  `role="status"` banner («Skills are a registry; they run once an orchestrator agent exists
  (S6b).») ABOVE the list — every skills-adjacent UI must keep stating this honestly until S6b
  actually ships a runtime consumer; don't let a future edit quietly drop the banner while the
  registry is still non-executable.

## Idea research flow — `components/idea/*`, untrusted-banner reuse (S-IDEA)

- **No new top-level view:** the research flow hangs entirely off the shipped «Ideas» surface
  (`IdeasList` in the `ProjectPanel` «Ideas» tab AND the project-less idea inbox) — unlike S-EXT's
  `"ext"` view, S-IDEA does not widen the `view` union. Four new components live under
  `src/components/idea/`: `ResearchRunDialog`, `ResearchPane`, `FormInsightDialog`,
  `SpawnProjectFromIdea` — each a focused modal/panel `IdeasList` opens per idea, not a
  tab-selection pattern like `ProjectPanel`/`ExtPanel`.
- **`ResearchRunDialog`:** picks a connected+enabled MCP server (`McpListTools` populates the tool
  picker once a server is chosen) → an owner-supplied args JSON field → a spend-approval preflight
  panel (`TrustListPolicies` for the effective scope + a fixed "cost usually unknown until after
  the call" note) → «Run» fires `researchStartRun`. The trust layer's hard caps are
  unchanged — a cap breach at invoke time surfaces as the run reaching `failed{policy_cap_exceeded}`
  in `ResearchPane`, not a dialog-time rejection (the preflight is advisory, not enforcing).
- **`ResearchPane` reuses the S-EXT untrusted-artifact banner verbatim, does NOT reinvent it:** a
  `done` run's artifact view calls the exact same artifact-rendering path `ArtifactsTab`/
  `ToolsBrowser` use (fetch via `mcpGetArtifact`, render content + the fixed «unverified
  data» banner unconditionally — every `mcp_artifact` is `is_untrusted=1` by construction, same
  contract as the Extensions-view convention above). **Never build a second untrusted-banner
  component** — if a research-specific artifact treatment is ever needed, extend the shared one,
  don't fork it.
- **The research pane is a STATUS list, not a stream — say so, don't fake it:** `ResearchPane`
  renders `pending|running|done|failed` badges per run (driven by the `ResearchRunsChanged` push,
  never polling) — there is no token-by-token rendering, because `mcp::invoke::call_tool` is
  request/response in the shipped connect-per-call model (BL-70 tracks the persistent-session
  architecture a real streaming pane would need). A `failed` run's card always offers «form
  insight without research» so the owner path never dead-ends on a research failure (Q8 honest
  degradation, same discipline as every other orchd-down/failure surface).
- **`FormInsightDialog`'s fit-context panel is read-only display, not a new editor:** it renders
  the project's goals (with `metric_refs` as owner-declared strings, no real metric timeseries —
  that's S8) plus a `GraphNeighborhood` read rooted at the idea/insight, reusing the S4 graph
  components' rendering, not a bespoke graph widget. The owner sets `fit_verdict`/`fit_reasoning`
  beside it — there is no agent-computed suggestion anywhere in this dialog (S6a is not built;
  don't add a "suggested verdict" placeholder that implies one exists).
- **`SpawnProjectFromIdea` is pure orchestration, no new IPC wrapper of its own:** it calls the
  three EXISTING wrappers in order (`pickFolder` → `createWorkspace` (sessiond) →
  `orchdCreateProject` → `orchdSetIdeaProject`) — resist the temptation to add a combined
  "spawn project" command; the multi-step UI IS the abstraction (S-IDEA spec D6, no new orchd
  verb).
- **Honest degradation, unchanged discipline:** every mutating control across the idea research
  flow (start-run, form-insight, accept/archive, form-task, spawn-project) is `disabled={orchdDown}`
  — same test pattern as the Extensions view: populate the form first, flip `orchdDown`, assert
  `disabled` AND assert the wrapper is never called on a click (userEvent, not `fireEvent`).

## Frontend reliability (S-POLISH P3)

Cross-cutting reliability contracts every mutating/read surface now obeys. Added by S-POLISH P3
(BL-92..97, D3/D6/D8) — see `docs/superpowers/plans/2026-07-16-s-polish.md`. These extend, never
replace, the honest-degradation discipline above.

- **`useSubmitGuard` on EVERY mutating submit (`src/hooks/useSubmitGuard.ts`, spec D6):** a rapid
  double click/Enter on a create/connect/run/set control fires the handler twice before React
  re-renders `disabled` — producing a duplicate row, a duplicate external call, or duplicate spend
  (findings E-08/F-08/G-08/H-01/J-03..05, P-19). The hook is the single shared fix: `const
  {submitting, guard} = useSubmitGuard(); const submit = guard(handler);` and the control renders
  `disabled={… || submitting}`. The re-entry lock is a `useRef` (synchronous — blocks the
  same-tick second call, which the batched `submitting` state cannot), and `submitting` toggles in
  a `finally` so a rejected handler still releases the lock. `guard` does NOT swallow errors — each
  wrapped handler keeps its own `try/catch → showToast`. Every dialog/list with a mutating submit
  has a double-fire test (two rapid clicks → wrapper called once). Do not add a new mutating submit
  without wrapping it.
- **Toast is a FIFO queue with manual dismiss, not a single clobbered slot (`store.ts`
  `showToast`/`dismissToast` + `src/components/Toast.tsx`, BL-97/D8):** `showToast` APPENDS to
  `toastQueue` (capped at 5, drop-oldest) — a burst of failures shows each in turn instead of the
  last one erasing the rest. `toast` (the visible one) is always `toastQueue[0]`; it auto-advances
  after `TOAST_AUTO_DISMISS_MS` (4s) and can be advanced early via the close button (`dismissToast`).
  The auto-advance timer is closure state re-armed for each new head and cancelled when the queue
  drains, so a stale timer can never clear a later toast. **Test consequence:** a component test that
  asserts on `useAppStore.getState().toast` must reset `{toast: null, toastQueue: []}` before the
  action if an earlier action in the same test (or a prior test without a reset in `beforeEach`)
  already queued one — otherwise it reads the stale HEAD, not the message it just fired.
- **Persistent storage-degradation banner (`src/components/StorageBanner.tsx`, D3/BL-94):** reads the
  store's `storageStatus` (pulled once on connect + every `orchd://up`; the mode is a boot fact, no
  push) and renders a red-accent `role="alert"` for `in_memory_fallback` ("changes will NOT survive
  a restart") and `recovered_from_corruption` (names the quarantined path). `persistent` (or a
  not-yet-fetched `null`) renders NOTHING — the healthy path shows no chrome, and there is no dismiss
  (only a daemon restart into a healthy state clears it). Copy lives in `strings.storage.*`. The
  cancelled-orchd-upgrade dead-end has the sibling reopenable banner `OrchdUpgradeBanner` (BL-96).
- **Reconnect rehydrates EVERY live slice + research self-poll (`App.tsx` `onOrchdUp`,
  `ResearchPane`, BL-92/D8):** `orchd://up` fires on the initial connect AND every reconnect; during
  any outage every coarse `orchd://*-changed` push is lost, so `onOrchdUp` refetches the WHOLE live
  surface — projects; the open project's goals/tasks/ideas/insights/ruleset/graph; the Extensions
  slices (servers/artifacts/accounts/skills/policies/invocations); research runs for every idea
  currently holding runs; the global ruleset when its surface has been opened; and the
  storage-degradation status. Adding a new store slice means adding its `refresh*` here — a slice
  that reconnects stale is the exact BL-92 bug. Separately, `ResearchPane` self-polls
  `researchListRuns` every 2s WHILE a mounted run is non-terminal (`pending`/`running`) and stops the
  moment all are terminal — the terminal `orchd://research-runs-changed` push can be missed during an
  outage, which would strand a run's badge forever; `onOrchdUp` covers loaded-but-unmounted ideas,
  the self-poll covers the visible pane.
- **Empty-state vs loading vs failed — three DISTINCT states, never conflated (Tier-3, P-11..15):**
  a read surface must not let "still loading" or "load failed" masquerade as "genuinely empty".
  - *loading ≠ empty:* while the first fetch is in flight render a distinct placeholder, not the
    empty copy (`CommandStrip`'s `command-strip-loading` vs `command-strip-empty`).
  - *failed ≠ empty, and offer a retry:* a rejected fetch renders an inline retry affordance (and a
    toast), never `null` forever with no recovery path (`CommandStrip`'s `command-strip-failed` +
    retry; `ConnectorsTab`'s per-account `ops-load-failed` + `ops-retry`, distinct from a `ready`
    account with an empty op catalog).
  - *empty is honest, not blank:* a genuinely-empty result renders a calm dim placeholder
    (`WorkspaceSidebar`'s `sidebar-empty` at zero projects+workspaces; `FileTree`'s `file-row-empty`
    marker for an expanded directory that loaded with no entries, distinct from the `file-row`
    loading placeholder and from a failed load, which toasts).
- **A rejected inline edit REVERTS to the store value (P-27):** a row that holds an in-flight
  title/body edit as local state (`GoalTree`'s `GoalRow`, `IdeasList`'s `IdeaRow`) must revert that
  local draft to the store's copy when the save is rejected — the commit handler returns
  `Promise<boolean>` and the row does `if (!ok) setLocal(store.value)`. A stale edit left hanging
  never self-heals (the sync `useEffect([store.value])` only fires when the store value CHANGES,
  which a failed save does not). Mirror `GoalRow.commit` — do not leave a rejected edit on screen.
- **On-row error signal for a control with no optimistic flip (J-01):** a toggle/checkbox bound
  directly to the wire value (`ToolsBrowser`'s `tool-enabled` = `tool.enabled`, no optimistic flip)
  shows NO visible change on a rejected mutation — a bare toast (clobber-prone before the queue, and
  easy to miss) is not enough. Surface an inline `role="alert"` on the row itself
  (`tool-toggle-error-*`), cleared on the next attempt.
- **Consent denials point at their recovery (P-20):** a `Consent`-kind rejection (a stale/changed
  consent grant, from `mcpCallTool`/`connectorInvoke`) is only recoverable via `ConnectDialog`,
  which is reachable ONLY from the Servers tab. `isConsentError(e)` (`ipc/orchd.ts`) gates appending
  `strings.errors.consentRecovery` ("… Extensions → Servers → Connect.") to the surfaced message, so
  the toast/inline error names the path forward instead of dead-ending.
- **`describeError` must match the error union (P-16/P-17):** a sessiond native-picker/workspace
  round-trip rejects with a `CommandError` (`src-tauri/src/commands.rs`), NOT an orchd error — map it
  with the per-surface `describeCommandError` (which preserves the real message), never
  `describeOrchdError` (which flattens a sessiond error to a generic "unknown orchestrator error")
  and never a raw `e.message`. `FileTree`/`WorkspaceSidebar`/`CreateProjectDialog`/`SkillsTab` each
  keep an identical local `describeCommandError` copy (same independently-deployable-per-surface
  rationale as `describeFsError`).

## Tier-2 feature controls (S-POLISH P4)

The four owner-facing controls S-POLISH P4 added (O-3/O-4/O-5/O-7, spec D7 — see
`docs/superpowers/plans/2026-07-16-s-polish.md`). Each obeys the honest-degradation +
`useSubmitGuard` disciplines above; the notes here are the P4-specific shape only.

- **Archive / un-archive controls (O-3):** a project is no longer a one-way archive trap. The
  sidebar (`WorkspaceSidebar.tsx`) renders a collapsed, dimmed «Archived (N)» group below the active
  project groups (copy `strings.sidebar.archivedGroup`), toggled by a disclosure header. An archived
  project's Overview (`ProjectPanel.tsx`) shows a read-only `role="status"` banner
  (`strings.project.archivedBanner`, "This project is archived and read-only. Un-archive it to make
  changes.") with an «Un-archive» button calling `orchdUnarchiveProject`; the «Archive» control
  confirms first (`strings.project.archiveConfirm`). Both mutating buttons are
  `disabled={orchdDown || submitting}` and route through `useSubmitGuard`. Every OTHER mutating
  control inside an archived project stays disabled by the existing archived-project guard — the
  banner is the honest signal for WHY, not a new disable path.
- **`metric_refs` chip editor (O-4):** each `GoalTree` row (`GoalRow`) renders `goal.metricRefs` as
  a chip list plus an add input (`strings.goal.addMetricPlaceholder` = "+ metric"), persisting the
  row's FULL next array via `onMetricRefsChange → orchdUpdateGoal` (the shipped verb — no new IPC
  wrapper). It follows the SAME "chips render straight off the store value, no local optimistic copy"
  model as the row's title edit, so a rejected save simply leaves the store value on screen (the
  chips never diverge); an add of an already-present ref is deduped client-side to skip a redundant
  round-trip. Each chip has a remove affordance (`strings.goal.removeMetricAria(ref)`). Disabled
  while `orchdDown`.
- **Graph editor — node form / inline rename / edge kind (O-7):** the S4 `GraphCanvas` toolbar gains
  an add-node FORM (title + optional body inputs → `graphAddNode`, `strings.graph.addNode`), a
  LOCAL-node inline rename (double-click a local node → an inline title/body editor →
  `graphUpdateNode`, with Save/Cancel — `strings.graph.renameAria`), and edge-kind editing (select an
  edge → a kind dropdown, `strings.graph.edgeKindAria`, → the new `graphUpdateEdge`; an edge's
  rendered label IS its kind). Only LOCAL nodes/edges are editable — a cross-project ghost node/edge
  stays read-only (its edit belongs to its own project's canvas), consistent with S4's ghost-styling
  rule. Every mutating control is `disabled` while `orchd://down`; the search input stays live (a
  read).
- **OAuth provider dropdown + honest empty-state (O-5):** `ConnectorsTab.tsx`'s OAuth-begin flow
  replaces the old free-text provider field with a `<select>` (`strings.ext.connectors.oauthProviderAria`)
  populated once from `connectorListProviders()`. When the config-backed registry is empty the tab
  shows the honest empty-state `strings.ext.connectors.noProviders` ("No OAuth providers configured
  — add one in oauth_providers.json (see runbook).") and the «begin OAuth» button stays disabled —
  never a dead dropdown that silently fails on submit. Provider NAMES only reach the UI (the backend
  never sends a `client_id`/secret/URL). The «add API key» path is unaffected — it needs no provider
  registry. Disabled (like every ext mutating control) while `orchdDown`.

## Testing contract (per frontend slice)

Three layers, all required:
1. **Unit** — store slices / pure logic (vitest).
2. **Component-integration** — components against the REAL store + REAL `TerminalManager`
   (mock only the IPC boundary). This is the layer that caught the A1 dead-pane bug; every slice
   must cover its primary user flow here (e.g. «create two X, switch between them»).
3. **One GUI smoke** per slice — a real-DOM happy path.

Tooling: vitest + Testing Library (jsdom) as used today; pin any new tool in the slice's spec
before S2 starts.
