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

## Testing contract (per frontend slice)

Three layers, all required:
1. **Unit** — store slices / pure logic (vitest).
2. **Component-integration** — components against the REAL store + REAL `TerminalManager`
   (mock only the IPC boundary). This is the layer that caught the A1 dead-pane bug; every slice
   must cover its primary user flow here (e.g. «create two X, switch between them»).
3. **One GUI smoke** per slice — a real-DOM happy path.

Tooling: vitest + Testing Library (jsdom) as used today; pin any new tool in the slice's spec
before S2 starts.
