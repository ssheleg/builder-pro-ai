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
  state and renders exactly one child per tab (Серверы/Инструменты/Коннекторы/Журнал/Артефакты/
  Навыки — `src/components/ext/*.tsx`), same shape as `ProjectPanel`'s Обзор/Цели/Идеи/…
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
  confirming is simpler and honest) — `Подключиться` grants `TrustGrantConsent(id, "connect")`
  THEN calls `mcpConnect`, in that order, since `mcpConnect` is trust-gated and rejects with
  `Error{Consent}` until the grant exists. A failure (network/consent/policy) is shown IN-dialog
  (`role="alert"`) via `describeOrchdError`, not just a toast. **`ServersTab`'s transport picker
  is fixed at `"http"` for now** — a `"stdio"` option is present but disabled ("скоро"): the
  backend's distinct `stdio_exec` consent kind (a different fingerprint scheme, binary-hash not
  URL) is fully built (`crates/orchd/src/trust.rs`), but no UI flow can create a stdio server
  yet, so there is no separate stdio-exec consent dialog to document — don't assume `ConnectDialog`
  covers it until a future task wires up the stdio transport picker.
- **Untrusted-result banner:** any surface that renders an MCP tool result or a connector-invoke
  result (`ToolsBrowser`'s invoke-result panel, `ArtifactsTab`'s per-row viewer) shows a fixed
  «непроверенные данные» banner — unconditional for any result at all (every `mcp_artifact` is
  `is_untrusted=1` by construction, S-EXT spec D9), never computed from response content. Treat
  this the same way as the graph canvas's orphan/ghost styling: a static, tested badge driven by
  a boolean flag from the wire, not inferred client-side.
- **Skills tab's plumbing-only banner:** `SkillsTab` renders a fixed, unconditional
  `role="status"` banner («Навыки — это реестр; они исполняются, когда появится агент-оркестр
  (S6b).») ABOVE the list — every skills-adjacent UI must keep stating this honestly until S6b
  actually ships a runtime consumer; don't let a future edit quietly drop the banner while the
  registry is still non-executable.

## Testing contract (per frontend slice)

Three layers, all required:
1. **Unit** — store slices / pure logic (vitest).
2. **Component-integration** — components against the REAL store + REAL `TerminalManager`
   (mock only the IPC boundary). This is the layer that caught the A1 dead-pane bug; every slice
   must cover its primary user flow here (e.g. «create two X, switch between them»).
3. **One GUI smoke** per slice — a real-DOM happy path.

Tooling: vitest + Testing Library (jsdom) as used today; pin any new tool in the slice's spec
before S2 starts.
