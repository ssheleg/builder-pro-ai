import { create } from "zustand";
import { readTheme, setThemePref, type Theme } from "../ui/theme";
import { classifyError, scrubSecrets, pushCapped, DIAG_CAP, type DiagEvent } from "../ipc/diag";
import { powerSetEnabled, powerSyncSessions, type PowerStatus } from "../ipc/power";
import { removeWorkspace as removeWorkspaceCmd } from "../ipc/commands";
import { statsUsage, statsGit } from "../ipc/stats";
import { strings } from "../strings";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";
import type { FsEntry } from "../ipc/fs";
import type {
  Account,
  AuditRow,
  DocMeta,
  DocView,
  DomainTask,
  Goal,
  GraphView,
  Idea,
  Insight,
  McpArtifact,
  McpInvocation,
  McpServer,
  McpTool,
  Policy,
  Project,
  ResearchRun,
  RuleScope,
  RuleSetView,
  Skill,
  Stage,
  StorageStatus,
  SupervisorConfig,
  Workflow,
  WorkflowScope,
} from "../ipc/orchd-types";
import {
  orchdGetDoc,
  orchdGetRuleset,
  orchdGraphListProject,
  orchdListDocs,
  orchdListGoals,
  orchdListIdeas,
  orchdListInsights,
  orchdListProjects,
  orchdListTasks,
  mcpListServers,
  mcpListTools,
  mcpListArtifacts,
  mcpListInvocations,
  connectorListAccounts,
  skillList,
  trustListPolicies,
  trustListAudit,
  researchListRuns,
  orchdStorageStatus,
  orchdListWorkflows,
  orchdUpsertWorkflow,
  orchdDeleteWorkflow,
  describeOrchdError,
  isNotFoundError,
} from "../ipc/orchd";

/**
 * Global app state (spec §12). METADATA ONLY — PTY bytes never enter this store;
 * they are written straight to xterm via the terminal Channel (see terminal-manager,
 * Task 21).
 *
 * `upsertSession`/`upsertWorkspace` are idempotent by id: the daemon broadcasts
 * `session://created` to ALL clients including the create originator, so a duplicate
 * upsert of the same (or updated) record must be harmless — insert-or-replace, never
 * throw or duplicate.
 */
export interface AppState {
  sessions: Record<SessionId, SessionMeta>;
  workspaces: Record<WorkspaceId, Workspace>;
  activeSessionId: SessionId | null;
  daemonConnected: boolean;
  /**
   * Honest daemon state (Pv2 §6.2-6.3): set `true` by `daemon://incompatible`, which is FATAL
   * (the client's connection task has exited and will NOT reconnect, unlike a plain disconnect).
   * Stays `true` until the app restarts (a successful upgrade resets everything via
   * `app.restart()`) — Cancel on the upgrade dialog must NOT clear it, or `DaemonBanner` would
   * revert to its "reconnecting…" copy, which would be a lie.
   */
  daemonIncompatible: boolean;
  /** Pure UI visibility for `UpgradeDialog`. `true` when the event fires; `false` on Cancel;
   * re-openable from `DaemonBanner`'s action. Independent of `daemonIncompatible` (see above). */
  upgradeDialogOpen: boolean;
  /**
   * Honest failure surface for the upgrade flow (finding [13], spec §6.2.4): `upgradeDaemon()`
   * never resolves on the happy path (the daemon restart kills this webview), but a REJECTED
   * promise (e.g. `CommandError::UpgradeFailed` from a TCC/MDM-denied `launchctl kickstart`) must
   * not vanish silently. `null` = no error to show. Cleared whenever the dialog (re)opens fresh
   * (`setUpgradeDialogOpen(true)`) or the user retries, so a stale error never lingers past a new
   * attempt.
   */
  upgradeError: string | null;
  /**
   * Set `true` only after the FIRST successful hydrate (`list_sessions`/`list_workspaces` both
   * resolving) — finding [14]: while `false`, `sessions` may simply not have been populated yet
   * (e.g. the client slot is `None` at boot-incompatible, so hydrate can never succeed), and any
   * "N live sessions" count derived from the store would silently understate reality. Consumers
   * (e.g. `UpgradeDialog`) must branch on this flag before trusting a session count. Never reset
   * back to `false` once true (a later disconnect doesn't un-hydrate the snapshot already held).
   */
  hydrated: boolean;

  /**
   * Top-level navigation (spec §6.6/§6.2/§10, S-EXT §8): `"home"` is the attention-first Home
   * view over ALL terminals across workspaces; `"workspace"` is the existing per-workspace
   * terminal layout; `"project"` is the S3 project panel (`openProject`, T18); `"ext"` is the
   * S-EXT Extensions panel (`ExtPanel`, T8) — MCP servers/tools/connectors/skills management;
   * `"workflows"` is the SW1 workflow-authoring library (`WorkflowsView`) — reusable
   * workflow-as-data authoring (config only, no runtime — S6b). Defaults to `"home"` — the owner's
   * daily loop starts there, never mid-workspace.
   */
  view: "home" | "workspace" | "project" | "ext" | "inbox" | "stats" | "workflows";

  /**
   * File-explorer slice (spec §6.6/§6.4). Every keyed map here uses the SAME key format:
   * `` `${root}\t${rel}` `` (tab-separated — `rel` itself may legitimately contain `/`, so a
   * tab avoids any ambiguity a `/`-joined key would have between a root boundary and a path
   * separator). `rel === ""` addresses the root directory itself.
   */
  /** Which directories are expanded in the tree, keyed `` `${root}\t${rel}` ``. A `Record` of
   * `true` (not `boolean`) so "expanded" is exactly "key present" — no stale `false` entries to
   * prune. */
  expanded: Record<string, true>;
  /** Lazily-fetched `listDir` results, keyed `` `${root}\t${rel}` ``. Absence means "not yet
   * fetched" (or invalidated) — never distinguished from an empty directory by anything other
   * than key presence. */
  treeCache: Record<string, FsEntry[]>;
  /** The file currently shown in the preview pane, or `null` when nothing is selected. */
  selectedFile: { root: string; rel: string } | null;
  /** Whether gitignored entries are shown (dimmed) in the tree. Defaults to `false` (spec §4.2:
   * ignored entries omitted by default). */
  showIgnored: boolean;
  /** Right-rail (files) visibility. Defaults to `false` (collapsed). */
  filesRailOpen: boolean;
  /** `true` while the live watch is paused after an `fs://watch-error` (spec §5/§7): the UI shows
   * a "live updates paused — refresh" affordance. Cleared on reactivation. */
  watchPaused: boolean;
  /**
   * Per-directory invalidation epochs (FS-8), keyed by the SAME `` `${root}\t${rel}` `` format as
   * `treeCache`. `invalidateDirs` BUMPS the epoch of every affected key (in addition to dropping
   * the cached listing); `FileTree` captures the epoch before starting a `listDir` and applies the
   * response ONLY when it still matches — a listing that was invalidated mid-flight is dropped and
   * re-fetched instead of landing as a stale cache entry (previously the invalidation was LOST
   * whenever the fetch was already in flight: there was no cache entry to drop yet and the
   * `fetchingRef` guard suppressed the refetch).
   */
  treeEpochs: Record<string, number>;

  /**
   * The CURRENTLY-VISIBLE toast message — the head of `toastQueue`, or `null` when the queue is
   * empty (design-system.md Toast atom, spec §7 "honest error surface": every async failure is a
   * toast with the mapped human message, never console-only). Kept in lockstep with
   * `toastQueue[0]` on every mutation so `<Toast/>` (`src/components/Toast.tsx`) can stay a pure
   * reader of this one field.
   */
  toast: string | null;
  /**
   * FIFO toast queue (BL-97, spec D8): `showToast` APPENDS (no longer clobbers) so a burst of
   * failures is shown one after another instead of only the last surviving. Capped at
   * `TOAST_QUEUE_CAP` (drop-oldest) so a runaway producer can never grow it unboundedly. The head
   * (`toastQueue[0]`) is the visible toast (`toast`); it auto-advances every `TOAST_AUTO_DISMISS_MS`
   * and can be advanced early via `dismissToast` (the manual close button).
   */
  toastQueue: string[];
  /**
   * The visible toast's TONE (FE-6): `"error"` (default — the honest-failure accent,
   * `var(--danger)`) or `"success"` (a positive confirmation, `var(--ok)`). Kept in lockstep with
   * `toast` exactly like `toastQueue` is, and rendered by `<Toast/>` as the left-edge accent.
   */
  toastTone: ToastTone;
  /** Parallel FIFO of the queued toasts' tones — index-for-index aligned with `toastQueue`
   * (`toastTone` is `toastToneQueue[0]`). Kept as a separate queue so the message queue's
   * `string[]` shape (and every existing reader/test of it) is untouched. */
  toastToneQueue: ToastTone[];

  /**
   * App-domain slice (spec §10, S3 T13): projects/goals/ideas/insights/tasks/rulesets live in
   * `bpa-orchd`, a SECOND daemon independent of sessiond — this slice is invalidation-driven
   * (D6: coarse `orchd://*-changed` pushes tell the frontend WHAT changed; the matching `refresh*`
   * action below re-fetches that list wholesale from `./orchd.ts`, replacing it — no client-side
   * merge/patch of individual rows).
   */
  /** The project currently open in the project panel (T18), or `null` on Home/workspace views.
   * Set by `openProject`. */
  activeProjectId: string | null;
  /** Every project (spec §5.1: `orchd_list_projects` has no filter — always the whole table).
   * Replaced wholesale by `refreshProjects`. */
  projects: Project[];
  /** A project's goal TREE (D5: full tree, not just top-level), keyed by `projectId`. Absence
   * means "not yet fetched" — same convention as `treeCache`. Replaced per-key by
   * `refreshGoals(projectId)`; a `GoalsChanged{projectId}` push never touches any OTHER project's
   * entry. */
  goalsByProject: Record<string, Goal[]>;
  /** Every idea, across every project (ideas are NOT split per project in this slice — the
   * ⌘K quick-capture inbox and Ideas panel filter client-side). Replaced wholesale by
   * `refreshIdeas`. */
  ideas: Idea[];
  /** Every insight, across every project. Mirrors `ideas` exactly. Replaced wholesale by
   * `refreshInsights`. */
  insights: Insight[];
  /** A project's flat task list (subtasks included, `parentId`-linked), keyed by `projectId`.
   * Mirrors `goalsByProject` exactly — absence means "not yet fetched", replaced per-key by
   * `refreshTasks(projectId)`. */
  tasksByProject: Record<string, DomainTask[]>;
  /** An idea's research runs, newest-first (S-IDEA §5/§7/§10, task T6), keyed by `ideaId`.
   * Mirrors `goalsByProject`/`tasksByProject` exactly — absence means "not yet fetched", replaced
   * per-key by `refreshResearchRuns(ideaId)`; a `ResearchRunsChanged{ideaId}` push never touches
   * any OTHER idea's entry. */
  researchRunsByIdea: Record<string, ResearchRun[]>;
  /** A project's knowledge graph (S4 §7), keyed by `projectId`. Mirrors `goalsByProject`/
   * `tasksByProject` exactly — absence means "not yet fetched", replaced wholesale per-key by
   * `refreshGraph(projectId)`; a `GraphChanged{projectId}` push never touches any OTHER project's
   * entry. */
  graphByProject: Record<string, GraphView>;
  /** RuleSet views keyed `` `global` `` (the one global ruleset) or `` `project:${id}` `` (a
   * single project's ruleset) — mirrors `orchd_get_ruleset`'s `(scope, projectId)` pair collapsed
   * into one string key. Replaced per-key by `refreshRuleset(key)`. */
  rulesets: Record<string, RuleSetView>;
  /** A project's doc list rows (SCN-054: name + last-modified, name-ordered by the daemon),
   * keyed by `projectId`. Mirrors `goalsByProject`/`tasksByProject` exactly — absence means "not
   * yet fetched" (the Docs tab's loading state), replaced per-key by `refreshDocs(projectId)`; a
   * `DocsChanged{projectId}` push never touches any OTHER project's entry. */
  docsByProject: Record<string, DocMeta[]>;
  /** Open doc views (SCN-054: the Docs tab's editor half — content + healthy/changed/lost file
   * state), keyed by [`docViewKey`]'s `` `${projectId}/${name}` `` (`/` can never appear in a
   * validated doc name, so the key is unambiguous). Mirrors `rulesets`' per-key convention;
   * replaced per-key by `refreshDoc(projectId, name)`, DROPPED per-key when a re-fetch reports
   * the doc no longer exists (deleted by another client — see `refreshDoc`'s doc). */
  docViews: Record<string, DocView>;

  /**
   * MCP slice (S-EXT §8, T8: the Extensions view's Servers/Tools tabs). Mirrors the app-domain
   * slice above exactly — invalidation-driven (D6: coarse `orchd://mcp-*-changed` pushes tell the
   * frontend WHAT changed; the matching `refresh*` action re-fetches wholesale/per-key from
   * `./orchd.ts`, replacing it — no client-side merge/patch).
   */
  /** Every MCP server (global scope — `mcpListServers(null)`; Phase 1's Extensions view has no
   * per-project server list yet). Replaced wholesale by `refreshMcpServers`, mirrors `projects`. */
  mcpServers: McpServer[];
  /** A server's cached tool list (from `mcp_tool`, refreshed on connect/`list_changed`), keyed by
   * `serverId`. Absence means "not yet fetched" — same convention as `goalsByProject`/
   * `tasksByProject`. Replaced per-key by `refreshMcpTools(serverId)`; a `McpToolsChanged
   * {serverId}` push never touches any OTHER server's entry. */
  mcpToolsByServer: Record<string, McpTool[]>;
  /** Every durable MCP artifact, unfiltered (mirrors `ideas`/`insights`'s whole-store
   * convention — the Artifacts tab filters client-side). Replaced wholesale by
   * `refreshMcpArtifacts`. */
  mcpArtifacts: McpArtifact[];
  /** Every connector account (S-EXT §8, T13b: the Extensions/Connectors tab). Mirrors
   * `mcpServers`'s whole-store, un-scoped convention exactly (`connectorListAccounts` has no
   * filter either) — replaced wholesale by `refreshAccounts`. */
  accounts: Account[];
  /** Every skill (global scope — `skillList(null)`; S-EXT §8, D11, T17: the Extensions/
   * Skills tab. PLUMBING ONLY — no runtime consumer until S6b). Mirrors `mcpServers`'s
   * whole-store, un-scoped convention exactly — replaced wholesale by `refreshSkills`. */
  skills: Skill[];
  /** Every MCP invocation, unfiltered (S-EXT §8, T18: the Extensions/Log tab). Mirrors
   * `mcpArtifacts`'s whole-store, un-scoped convention exactly — replaced wholesale by
   * `refreshInvocations`. */
  invocations: McpInvocation[];
  /** Every `audit_log` row, newest-first (S-EXT §4/§6/§8, BL-22, T18: the Extensions/
   * Log tab's audit view). Replaced wholesale by `refreshAuditRows`. */
  auditRows: AuditRow[];
  /** Every configured spend/rate policy (S-EXT §4/§6/§8, BL-22, T18: the Extensions/Log
   * tab's policy editor). Replaced wholesale by `refreshPolicies`. */
  policies: Policy[];

  /**
   * First-fetch-completed flags (UX-1): every list slice above initializes EMPTY, so "the fetch
   * has not completed yet" and "the fetch completed and the list is genuinely empty" were
   * indistinguishable — a list with real rows flashed its EmptyState copy until the first
   * `refresh*` landed. The matching flag flips `true` when the FIRST fetch for that slice SETTLES
   * — success OR failure (a failed fetch surfaces via toast/diag; leaving the flag down would pin
   * the surface on an eternal loading row, which is the worse lie). Keyed slices mirror their data
   * map (`Record<key, true>` — presence = fetched, same convention as `expanded`). Flags never
   * reset to `false` once set: they mean "the first fetch completed", not "the data is fresh" —
   * later invalidation re-fetches replace the data wholesale and a transient re-fetch must NOT
   * re-empty the list into a loading flash. Consumers render a loading row until the flag is set
   * (the `DocsPanel` docs-loading/docs-empty split pattern).
   */
  projectsFetched: boolean;
  goalsFetched: Record<string, true>;
  ideasFetched: boolean;
  insightsFetched: boolean;
  tasksFetched: Record<string, true>;
  researchRunsFetched: Record<string, true>;
  mcpServersFetched: boolean;
  mcpArtifactsFetched: boolean;
  accountsFetched: boolean;
  skillsFetched: boolean;

  /**
   * Workflow-authoring slice (SW1, docs/ux/plans/2026-07-24-workflow-authoring.md). Every reusable
   * workflow-as-data definition (`orchdListWorkflows(null, null)` — all scopes; the library filters
   * global/project client-side). Mirrors `skills`/`mcpServers`'s whole-store, invalidation-driven
   * convention exactly — replaced wholesale by `refreshWorkflows`, which the `orchd://workflows-
   * changed` push (App.tsx) fires. AUTHORING/CONFIG ONLY — no runtime consumer until the S6b
   * executor lands (`RunWorkflowPicker.tsx`'s honest pending note). */
  workflows: Workflow[];

  /** Honest orchd connectivity (spec §9/§11, mirrors sessiond's `daemonConnected` inverted):
   * `true` while the `orchd://down` event is the most recent connection-state signal seen, `false`
   * once `orchd://up` fires. Every domain surface shows the shared "Orchestrator unavailable"
   * banner + disables mutating controls while this is `true` (T19). */
  orchdDown: boolean;
  /** Set by `orchd://incompatible` (FATAL, like `daemonIncompatible` — the orchd client's
   * connection task has exited and will not reconnect on its own; never auto-clears). */
  orchdIncompatible: boolean;
  /** Pure UI visibility for the (T19-generalized) upgrade dialog's orchd branch. Independent of
   * `orchdIncompatible` (mirrors `upgradeDialogOpen`'s relationship to `daemonIncompatible` — see
   * that field's doc above for the same honesty invariant: Cancel must not clear
   * `orchdIncompatible`). */
  orchdUpgradeDialogOpen: boolean;

  /** The daemon's storage-degradation mode (spec D3, BL-94), or `null` before the first fetch.
   * Fixed at boot, so it is pulled once on connect and on every `orchd://up` reconnect (no push).
   * `StorageBanner` renders a persistent honest banner for the two non-`persistent` modes. */
  storageStatus: StorageStatus | null;

  /** Insert or replace a session by `meta.id`. Idempotent. */
  upsertSession: (meta: SessionMeta) => void;
  /** Delete a session; clears `activeSessionId` if it pointed at the removed session. */
  removeSession: (id: SessionId) => void;
  /**
   * Apply a `session://state-changed` payload: updates `lifecycle`/`waitingForInput`/`cwd`
   * on the matching session. No-op if the session isn't in the map (e.g. a stale/late
   * event after removal).
   */
  setLifecycle: (p: StateChangedPayload) => void;
  /**
   * Apply a `session://exited` payload: sets `isActive:false`, clears `waitingForInput` (a
   * finished process is never waiting for input — the honest state for stats/StatusDot/HomeView;
   * the live event carries no such field, so a session that exited/crashed while blocked on stdin
   * must not keep a stale `true` forever), and an `{kind:"exited"}` lifecycle carrying the exit
   * code/signal. No-op if the session isn't in the map.
   */
  markExited: (p: ExitedPayload) => void;
  setDaemonConnected: (connected: boolean) => void;
  setDaemonIncompatible: (v: boolean) => void;
  setUpgradeDialogOpen: (v: boolean) => void;
  /** Set the upgrade-failure message (or clear it with `null`). See `upgradeError` doc above. */
  setUpgradeError: (v: string | null) => void;
  /** Set `true` after the first successful hydrate. See `hydrated` doc above. */
  setHydrated: (v: boolean) => void;
  /** Insert or replace a workspace by `ws.id`. Idempotent. Also the `workspace://updated`
   * listener's handler (spec §6.6): that event's payload IS a `Workspace`, so wiring it is
   * literally `onWorkspaceUpdated(upsertWorkspace)` — no separate action needed. */
  upsertWorkspace: (ws: Workspace) => void;
  /**
   * Drop a workspace AND every session that belonged to it from the slice (SCN-058), clearing
   * `activeSessionId` if it pointed at one of those sessions. Idempotent — an id that is already
   * gone is a no-op, which is what lets the local `removeWorkspace` path and the
   * `workspace://removed` broadcast (which reaches this client too, and every other window) both
   * call it without fighting each other. Pure state: it neither talks to the daemon nor decides
   * which view to show — the removal's caller owns the "was I looking at it?" fallback, because
   * the active WORKSPACE is App-level UI selection, not store data.
   */
  dropWorkspace: (workspaceId: WorkspaceId) => void;
  /**
   * Remove a workspace permanently (SCN-058): daemon round-trip first, then `dropWorkspace` on
   * success. REJECTS with the raw `CommandError` when the daemon refuses (down, unknown id),
   * leaving the slice completely untouched so the row stays exactly where it was — the caller
   * surfaces the honest toast.
   */
  removeWorkspace: (workspaceId: WorkspaceId) => Promise<void>;
  setActiveSession: (id: SessionId | null) => void;

  /** Switch the top-level view. See `view`'s doc above. */
  setView: (
    v: "home" | "workspace" | "project" | "ext" | "inbox" | "stats" | "workflows",
  ) => void;
  /** Set (`open=true`) or clear (`open=false`) one directory's expanded flag. */
  setExpanded: (root: string, rel: string, open: boolean) => void;
  /** Insert or replace one directory's cached listing. */
  cacheDir: (root: string, rel: string, entries: FsEntry[]) => void;
  /**
   * Apply an `fs://changed` batch (spec §5) to `treeCache` for `root` — a POINT REFRESH, never a
   * collapse: `expanded` is deliberately left UNTOUCHED, so a directory the owner had open stays
   * open and `FileTree`'s own effect (spec §6.4) re-fetches it since it's now uncached, with no
   * explicit re-expand click needed. Clearing `expanded` here would collapse the whole tree on
   * every file an agent writes, which is the opposite of the intended live-refresh UX. `rels` is
   * the event's `changedRelPaths`, treated as literal directory keys to drop (the caller is
   * responsible for mapping a changed FILE path to its containing directory's `rel` first — this
   * action itself does no path arithmetic). `rels === ["*"]` (the watcher's overflow sentinel,
   * spec §5: >500 distinct paths in one debounced batch) drops EVERY `treeCache` entry under
   * `root` — i.e. "refresh everything expanded under this root" — while entries for every OTHER
   * root are left untouched. Otherwise, only the exact `` `${root}\t${rel}` `` keys named in
   * `rels` are dropped.
   */
  invalidateDirs: (root: string, rels: string[]) => void;
  /** Set (or clear, with `null`) the file shown in the preview pane. */
  setSelectedFile: (sel: { root: string; rel: string } | null) => void;
  /** Flip `showIgnored`. */
  toggleShowIgnored: () => void;
  /** Set the files right-rail's open/closed state. */
  setFilesRailOpen: (b: boolean) => void;
  /** Set `watchPaused`. See its doc above. */
  setWatchPaused: (b: boolean) => void;

  /**
   * APPEND a toast to `toastQueue` (BL-97, spec D8 — no longer clobbers a still-showing one) and,
   * if a fresh toast became visible, start its `TOAST_AUTO_DISMISS_MS` auto-advance timer. See
   * `toastQueue`'s doc above. `<Toast/>` (`src/components/Toast.tsx`) is a pure reader of `toast`
   * — it never owns this timer itself, so the auto-advance fires even across a remount.
   * `tone` defaults to `"error"` (this atom exists to surface failures); pass `"success"` for a
   * positive confirmation (saved/created/copied) so it renders with the `var(--ok)` accent (FE-6).
   */
  showToast: (message: string, tone?: ToastTone) => void;
  /** Advance the queue by one (drop the visible head, show the next) — the manual close button's
   * action AND the auto-advance path. Reschedules the timer for the newly-visible toast, or
   * cancels it when the queue empties, so a stale timer can never clear a later toast. */
  dismissToast: () => void;

  /**
   * S-DIAG diagnostics ring (newest-first, capped at `DIAG_CAP`). Every recorded failure lands here
   * so the operator can RECONSTRUCT a cause after the transient toast is gone (`DiagnosticsPanel`
   * renders it). Secret-scrubbed at record time; survives only for the session (in-memory). */
  diagEvents: DiagEvent[];
  /**
   * Record a failure AND surface it: classify `e` (`./ipc/diag`), scrub its detail, push a
   * `DiagEvent`, `console.error` a structured breadcrumb, and `showToast` the human message — the
   * ONE place an async/IPC failure is both logged and shown. Returns the shown message so a caller
   * can reuse it (e.g. inline error text). `op` is the logical operation name (`"refreshProjects"`). */
  reportError: (op: string, e: unknown) => string;
  /** Clear the diagnostics ring (the panel's "Clear" action). */
  clearDiag: () => void;
  /** Record a React render crash caught by `ErrorBoundary` as a `kind:"render"` diag event (message
   * + scrubbed component stack), so a white-screen has a reconstructable cause. Does NOT toast — the
   * boundary already shows a full recovery card in place of the crashed subtree. */
  recordRenderCrash: (error: Error, componentStack: string) => void;

  /** Re-fetch `projects` wholesale from orchd (`orchd_list_projects` has no filter). The
   * `orchd://projects-changed` handler (App.tsx) and T18's project UI both call this directly —
   * a failure shows the mapped honest message as a toast (spec §7) rather than being swallowed. */
  refreshProjects: () => Promise<void>;
  /** Re-fetch ONE project's goal tree, replacing only `goalsByProject[projectId]` — every other
   * project's entry is left untouched (see `goalsByProject`'s doc above). */
  refreshGoals: (projectId: string) => Promise<void>;
  /** Re-fetch `ideas` wholesale (every project, unfiltered — see `ideas`'s doc above). */
  refreshIdeas: () => Promise<void>;
  /** Re-fetch `insights` wholesale. Mirrors `refreshIdeas` exactly. */
  refreshInsights: () => Promise<void>;
  /** Re-fetch ONE project's task list, replacing only `tasksByProject[projectId]`. Mirrors
   * `refreshGoals` exactly. */
  refreshTasks: (projectId: string) => Promise<void>;
  /** Re-fetch ONE idea's research runs, replacing only `researchRunsByIdea[ideaId]`. Mirrors
   * `refreshGoals`/`refreshTasks` exactly. */
  refreshResearchRuns: (ideaId: string) => Promise<void>;
  /** Re-fetch ONE project's knowledge graph, replacing only `graphByProject[projectId]`. Mirrors
   * `refreshGoals`/`refreshTasks` exactly. */
  refreshGraph: (projectId: string) => Promise<void>;
  /** Re-fetch one ruleset by its `rulesets` key (`` `global` `` or `` `project:${id}` `` — see
   * `rulesets`'s doc above), replacing only that key's entry. */
  refreshRuleset: (key: string) => Promise<void>;
  /** Re-fetch ONE project's doc list, replacing only `docsByProject[projectId]` (SCN-054).
   * Mirrors `refreshGoals`/`refreshTasks` exactly. */
  refreshDocs: (projectId: string) => Promise<void>;
  /** Re-fetch ONE doc's view, replacing only `docViews[docViewKey(projectId, name)]`. A
   * `NotFound` rejection DROPS the entry instead of toasting — that is the honest "this doc was
   * deleted by another client" signal a `DocsChanged` push refresh can race into, not an error
   * (see `isNotFoundError`'s doc, `ipc/orchd.ts`); every other rejection surfaces via
   * `reportError` like its siblings. */
  refreshDoc: (projectId: string, name: string) => Promise<void>;
  /** Open the project panel: sets `view: "project"` and `activeProjectId: id` (T18 renders the
   * panel itself; this task only owns the state transition). */
  openProject: (id: string) => void;

  /** Re-fetch `mcpServers` wholesale (`mcpListServers(null)` — global scope). Mirrors
   * `refreshProjects` exactly: try/catch -> `showToast(describeOrchdError(e))` on failure,
   * replace on success. */
  refreshMcpServers: () => Promise<void>;
  /** Re-fetch ONE server's cached tool list, replacing only `mcpToolsByServer[serverId]` — every
   * other server's entry is left untouched. Mirrors `refreshGoals`/`refreshTasks` exactly. */
  refreshMcpTools: (serverId: string) => Promise<void>;
  /** Re-fetch `mcpArtifacts` wholesale (no project/server filter — every artifact). Mirrors
   * `refreshIdeas`/`refreshInsights` exactly. */
  refreshMcpArtifacts: () => Promise<void>;
  /** Re-fetch `accounts` wholesale (`connectorListAccounts()` has no filter). Mirrors
   * `refreshMcpServers` exactly: try/catch -> `showToast(describeOrchdError(e))` on failure,
   * replace on success. */
  refreshAccounts: () => Promise<void>;
  /** Re-fetch `skills` wholesale (`skillList(null)` — global scope). Mirrors `refreshMcpServers`
   * exactly: try/catch -> `showToast(describeOrchdError(e))` on failure, replace on success. */
  refreshSkills: () => Promise<void>;
  /** Re-fetch `invocations` wholesale (`mcpListInvocations(null, null, null)` — no filter).
   * Mirrors `refreshMcpArtifacts` exactly, PLUS a trailing 300ms debounce (FE-1): `App.tsx` fires
   * this on every `orchd://mcp-invocation-logged` push, so bursts collapse into one trailing
   * fetch. The returned promise resolves when that fetch completes. */
  refreshInvocations: () => Promise<void>;
  /** Re-fetch `auditRows` wholesale (`trustListAudit(null)` — no cap). Mirrors
   * `refreshInvocations` exactly. */
  refreshAuditRows: () => Promise<void>;
  /** Re-fetch `policies` wholesale (`trustListPolicies()` has no filter). Mirrors
   * `refreshMcpServers` exactly. */
  refreshPolicies: () => Promise<void>;

  /** Re-fetch `workflows` wholesale (`orchdListWorkflows(null, null)` — all scopes; SW1). Mirrors
   * `refreshSkills` exactly: try/catch → `reportError` on failure, replace on success. */
  refreshWorkflows: () => Promise<void>;
  /** Create (`id: ""`) or save (a non-empty `id`) a workflow via `orchdUpsertWorkflow` (SW1). On
   * success upserts the returned definition into `workflows` by id for immediate feedback (the
   * `orchd://workflows-changed` push then re-fetches the whole list) and RESOLVES with it; a
   * daemon rejection propagates untouched so the editor keeps the owner's draft on screen and
   * surfaces the honest mapped toast itself. */
  upsertWorkflow: (params: {
    id: string;
    name: string;
    description: string;
    scope: WorkflowScope;
    projectId: string | null;
    defaultAgent: string;
    stages: Stage[];
    globalSkillIds: string[];
    supervisor: SupervisorConfig;
  }) => Promise<Workflow>;
  /** Delete a workflow via `orchdDeleteWorkflow` (SW1): daemon round-trip first, then drop it from
   * `workflows` on success. REJECTS with the raw error when the daemon refuses (down, unknown id),
   * leaving the slice untouched — the library surfaces the honest toast. */
  deleteWorkflow: (id: string) => Promise<void>;
  /** Re-fetch `storageStatus` (`orchdStorageStatus()` — spec D3, BL-94). Called on connect and on
   * every `orchd://up`. Mirrors `refreshMcpServers` exactly: try/catch -> toast on failure,
   * replace on success. */
  refreshStorageStatus: () => Promise<void>;
  /** Set `orchdDown`. See its doc above. */
  setOrchdDown: (v: boolean) => void;
  /** Set `orchdIncompatible`. See its doc above — never auto-clears, mirrors
   * `setDaemonIncompatible`. */
  setOrchdIncompatible: (v: boolean) => void;
  /** Set `orchdUpgradeDialogOpen`. See its doc above. */
  setOrchdUpgradeDialogOpen: (v: boolean) => void;

  /** Current theme preference (light / dark / system). Boot-applied in main.tsx; `setTheme`
   * persists + re-applies it to the document root (S-UXR B1). */
  theme: Theme;
  /** Persist + apply + store the theme preference. */
  setTheme: (t: Theme) => void;

  /**
   * Keep-awake slice (SCN-045, FLW-18): `enabled` = the persisted toggle (localStorage
   * `"bpa-keep-awake"`, default ON — mirrors `theme`'s FOUC-free localStorage persistence, see
   * `../ui/theme.ts`); `active` = the macOS power assertion is GENUINELY held right now (the
   * core's `SleepAsserter::is_held`, never the intent — spec §7 honesty); `error` = the current
   * OS acquire denial while a hold is still wanted (`null` otherwise), driving the pill's
   * failure state (`WorkspaceSidebar`).
   */
  keepAwake: { enabled: boolean; active: boolean; error: string | null };
  /**
   * Persist the toggle, push it into the core (`power_set_enabled`) and mirror the reconciled
   * `PowerStatus` back. A denial/rejection surfaces honestly via `reportError` (toast + Diag
   * record, SCN-045 "Errors & recovery") — never a silent fake "awake".
   */
  setKeepAwakeEnabled: (enabled: boolean) => Promise<void>;
  /**
   * Sync the live-session count into the core (`power_sync_sessions`; `App.tsx` calls this
   * whenever the number of `lifecycle.kind !== "exited"` sessions changes) and mirror the
   * reconciled status. An OS denial is surfaced ONCE per failure streak (not on every sync —
   * the count changes with normal session churn, and re-toasting an unchanged denial each time
   * would be pure noise), while `keepAwake.error` stays current on every call.
   */
  syncKeepAwake: (liveCount: number) => Promise<void>;

  /**
   * Stats view slice (SCN-052/053, FLW-20). Two independent sources with per-source honesty:
   * `usage` (Claude Code session-log scan; its own `error` field carries scan failure) and
   * `git` (per-root output stats; per-row `available:false` + reason). `gitError` is the
   * IPC-level failure for the git call as a whole; `usageError` likewise for usage — either
   * source failing must never blank the other (SCN-052 "Errors & recovery").
   */
  stats: {
    range: import("../ipc/stats").StatsRange;
    usage: import("../ipc/stats").UsageStats | null;
    git: import("../ipc/stats").GitStats[] | null;
    usageError: string | null;
    gitError: string | null;
    loading: boolean;
    /** Monotonic request token. Every `refreshStats`/`cancelStats` bumps it; a settled reply is
     * applied ONLY when it still matches — a slower, stale scan can never overwrite a newer range
     * (AUD-2026-07-23-25). */
    epoch: number;
    /** Unix ms at the last applied refresh completion — the "as of" fallback when the usage
     * source itself failed (git table is never left stampless, AUD-2026-07-23-13). */
    lastRefreshMs: number | null;
  };
  /** Switch the range pill and refetch (SCN-052 step 2). */
  setStatsRange: (range: import("../ipc/stats").StatsRange) => Promise<void>;
  /** Fetch both sources for the current range. Per-source failure capture — see `stats` doc. */
  refreshStats: () => Promise<void>;
  /** Abandon the in-flight scan (SCN-053 "slow scan → cancellable"): bump the epoch so the
   * pending reply is discarded, and clear `loading`. Idempotent when nothing is in flight. */
  cancelStats: () => void;
}

/** Key format shared by `expanded`/`treeCache`/`treeEpochs` — see their docs on `AppState` above. */
function fsKey(root: string, rel: string): string {
  return `${root}\t${rel}`;
}

/** The toast accent (FE-6): `"error"` is the default honest-failure surface (`var(--danger)`);
 * `"success"` marks a positive confirmation (`var(--ok)`). */
export type ToastTone = "error" | "success";

/**
 * The global ruleset scope's key is the literal `` `global` `` (spec §10: `rulesets`' key
 * format). Every OTHER key is a project scope, `` `project:${id}` `` — this parses one key back
 * into `orchd_get_ruleset`'s `(scope, projectId)` pair. A key that is neither `"global"` nor a
 * `"project:"`-prefixed string is treated as a project id verbatim (defensive default; every
 * caller in this codebase only ever constructs keys via the two documented forms).
 */
function parseRulesetKey(key: string): { scope: RuleScope; projectId: string | null } {
  if (key === "global") return { scope: "global", projectId: null };
  const projectId = key.startsWith("project:") ? key.slice("project:".length) : key;
  return { scope: "project", projectId };
}

/**
 * `docViews`' key format (SCN-054): `orchd_get_doc`'s `(projectId, name)` pair collapsed into
 * one string key, mirroring how `rulesets`' key collapses `(scope, projectId)`. `/` is safe as
 * the separator — the daemon's `validate_doc_name` rejects any name containing it, so the split
 * is unambiguous. Exported for `DocsPanel.tsx`/`App.tsx`, which read/refresh entries by the same
 * key the store writes.
 */
export function docViewKey(projectId: string, name: string): string {
  return `${projectId}/${name}`;
}

/**
 * The four session buckets, as an EXHAUSTIVE PARTITION (SCN-004/SCN-016) — see
 * [`partitionSessions`]. Every session in the input lands in exactly one array, so
 * `live + waiting + restored + exited === total` always holds.
 */
export interface SessionBuckets {
  /** Not exited, not waiting, and the daemon says the PTY is live. */
  live: SessionMeta[];
  /** Not exited and blocked on the owner (`waitingForInput`). */
  waiting: SessionMeta[];
  /**
   * Not exited, not waiting — but NO live PTY (`isActive === false`). This is what a session
   * rehydrated from the daemon's store looks like after a restart: sessiond hard-codes
   * `is_active:false`/`waiting_for_input:false` with an `atPrompt`/`running` lifecycle on restore
   * (`crates/sessiond/src/persistence.rs`), because the shell it describes is gone. Reporting one
   * of these as "live" would be a lie, and dropping it from every bucket (the pre-SCN-058 bug) hid
   * it from Home entirely — hence its own, honestly-named bucket.
   */
  restored: SessionMeta[];
  /** Finished — the lifecycle the daemon persisted or pushed on exit. */
  exited: SessionMeta[];
}

/**
 * THE definition of "live"/"waiting"/"restored"/"exited" for the whole frontend (SCN-004,
 * SCN-016): one function, so a label means the same thing on Home, on the workspace stat chips and
 * anywhere else that counts sessions. Before this existed, three surfaces each hand-rolled their
 * own three NON-EXHAUSTIVE predicates, and a cold-rehydrated session (`isActive:false`,
 * `waitingForInput:false`, lifecycle `atPrompt`) matched NONE of them — it was invisible in the
 * counts and absent from every Home section while its tab sat on screen.
 *
 * Order matters and is intentional: `exited` wins over everything (belt-and-suspenders against a
 * stale `waitingForInput:true` on a dead session — mirrors `StatusDot.dotStateOf`), then
 * `waiting` (a human is needed) over liveness, then a live PTY, and finally the honest remainder.
 * Because the branches are an if/else chain over one pass, the buckets cannot overlap and cannot
 * lose a session — the partition invariant is structural, not a convention.
 *
 * NOTE: `live` here deliberately EXCLUDES waiting sessions (they have their own bucket/label);
 * "how many sessions are open at all" is a different question — `App.tsx` computes that
 * separately for the keep-awake assertion (SCN-045) and names it so.
 */
export function partitionSessions(list: SessionMeta[]): SessionBuckets {
  const buckets: SessionBuckets = { live: [], waiting: [], restored: [], exited: [] };
  for (const meta of list) {
    if (meta.lifecycle.kind === "exited") buckets.exited.push(meta);
    else if (meta.waitingForInput) buckets.waiting.push(meta);
    else if (meta.isActive) buckets.live.push(meta);
    else buckets.restored.push(meta);
  }
  return buckets;
}

/** localStorage key for the keep-awake toggle (SCN-045). Values `"on"`/`"off"`; absence = the
 * default ON. Same synchronous, FOUC-free persistence path as the theme (`../ui/theme.ts`). */
const KEEP_AWAKE_STORAGE_KEY = "bpa-keep-awake";

/** Read the persisted keep-awake preference; defaults to ON (SCN-045 "default on"). Mirrors
 * `readTheme`'s try/catch: localStorage can throw in a locked-down webview. */
function readKeepAwakeEnabled(): boolean {
  try {
    const v = localStorage.getItem(KEEP_AWAKE_STORAGE_KEY);
    if (v === "on") return true;
    if (v === "off") return false;
  } catch {
    // localStorage unavailable — fall through to the default.
  }
  return true;
}

/** Persist the keep-awake toggle. Best-effort, mirrors `setThemePref`: a failed write only costs
 * persistence across restarts, never this session's behavior. */
function persistKeepAwakeEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(KEEP_AWAKE_STORAGE_KEY, enabled ? "on" : "off");
  } catch {
    // best-effort persistence; the core still reconciles this session.
  }
}

/** Best human-readable reason out of an unknown power-IPC rejection (the `power_*` commands
 * themselves are infallible — a rejection means the invoke/runtime layer broke). */
function describePowerFailure(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (e !== null && typeof e === "object") {
    const m = (e as { message?: unknown }).message;
    if (typeof m === "string" && m) return m;
  }
  return String(e);
}

/** How long the visible toast stays up before auto-advancing to the next (Toast atom, spec §7). */
const TOAST_AUTO_DISMISS_MS = 4000;

/** Trailing-debounce window for `refreshInvocations` (FE-1): `App.tsx` re-fires it on every
 * `orchd://mcp-invocation-logged` push, so bursts collapse into one trailing fetch. */
const INVOCATIONS_DEBOUNCE_MS = 300;

/** FIFO toast-queue cap (BL-97, spec D8): a runaway producer drops the OLDEST rather than growing
 * unboundedly, so the queue never holds more than this many pending notices. */
const TOAST_QUEUE_CAP = 5;

// Monotonic per-session id for diagnostics events (module scope so it survives store re-creation in
// a single session; not persisted). Not `Math.random`/`Date.now`-based so ids stay stable + ordered.
let diagSeq = 0;

export const useAppStore = create<AppState>((set, get) => {
  // Toast auto-advance bookkeeping (closure state, not store state — it's write-only plumbing,
  // like terminal-manager's attachGeneration guard). A single timer drives the visible head; it is
  // (re)started whenever a NEW toast becomes visible and cancelled when the queue drains, so it
  // can never advance a toast that has already been superseded.
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const clearToastTimer = (): void => {
    if (toastTimer !== undefined) {
      clearTimeout(toastTimer);
      toastTimer = undefined;
    }
  };

  // (Re)arm the auto-advance timer for the current head. On fire it advances the queue via
  // `dismissToast`, which itself re-arms for the next head (or cancels when the queue empties).
  const armToastTimer = (): void => {
    clearToastTimer();
    toastTimer = setTimeout(() => {
      toastTimer = undefined;
      get().dismissToast();
    }, TOAST_AUTO_DISMISS_MS);
  };

  // Keep-awake once-per-streak gate (SCN-045; closure state like `toastTimer` — write-only
  // plumbing, not renderable state): the failure reason last REPORTED (toast + Diag record).
  // `syncKeepAwake` fires on every live-count change, so an unchanged denial must not re-toast
  // on each session start/stop; a recovery (`error: null`) re-arms the gate so the NEXT streak
  // is reported again.
  let lastPowerError: string | null = null;

  // Surface a keep-awake failure honestly, once per streak: SCN-045's exact copy
  // ("keep-awake unavailable: {reason}") through the store's ONE reporting path (`reportError`:
  // toast + Diagnostics ring + console breadcrumb). Shaped as a `daemon`-kind error with the
  // `Power` code so `classifyError` files the Diag event under `kind: "Power"` and
  // `describeOrchdError` passes the message through verbatim.
  const reportPowerFailure = (op: string, reason: string): void => {
    if (reason === lastPowerError) return;
    lastPowerError = reason;
    get().reportError(op, {
      kind: "daemon",
      code: "Power",
      message: strings.power.keepAwakeFailed(reason),
    });
  };

  // Mirror a reconciled `PowerStatus` into the slice and route its error (if any) through the
  // once-per-streak gate. A clean status re-arms the gate — the streak is over.
  const applyPowerStatus = (op: string, status: PowerStatus): void => {
    set({ keepAwake: { enabled: status.enabled, active: status.active, error: status.error } });
    if (status.error !== null) reportPowerFailure(op, status.error);
    else lastPowerError = null;
  };

  // ── refresh* race guard (FE-1; closure state like `toastTimer` — write-only plumbing) ────────
  //
  // Every `refresh*` action below runs through `guardedRefresh(key, attempt)` — the `stats` slice's
  // epoch guard generalized. Per key: at most ONE fetch is in flight; a call arriving mid-flight
  // only marks the guard `dirty` (and bumps the epoch) instead of stacking a parallel invoke; when
  // the in-flight attempt settles, a dirty guard re-runs the attempt ONCE (trailing edge) so the
  // final state always comes from a fetch started after the last invalidation. An attempt applies
  // its response ONLY while its captured epoch is still current (`isCurrent()`) — a response that
  // was superseded mid-flight is dropped, so an out-of-order/slower stale reply can never clobber
  // fresher data. Keyed slices pass their natural key (`goals:${projectId}`, `doc:${key}`, …), so
  // two DIFFERENT projects/ideas/servers never block each other.
  const refreshGuards = new Map<string, { epoch: number; inFlight: boolean; dirty: boolean }>();

  // `refreshInvocations`' trailing-debounce bookkeeping (FE-1; closure state like `toastTimer`):
  // the pending trailing-edge timer plus the resolve callbacks of every caller collapsed into it.
  let invocationsTimer: ReturnType<typeof setTimeout> | undefined;
  let invocationsWaiters: Array<() => void> = [];

  const guardedRefresh = async (
    key: string,
    attempt: (isCurrent: () => boolean) => Promise<void>,
  ): Promise<void> => {
    let g = refreshGuards.get(key);
    if (!g) {
      g = { epoch: 0, inFlight: false, dirty: false };
      refreshGuards.set(key, g);
    }
    g.epoch += 1;
    if (g.inFlight) {
      // A fetch is already running — it predates this call's invalidation, so its (possibly
      // stale) response will be dropped by the epoch check; mark dirty and let ITS settle path
      // re-run the attempt instead of firing a parallel invoke.
      g.dirty = true;
      return;
    }
    g.inFlight = true;
    try {
      do {
        g.dirty = false;
        const attemptEpoch = g.epoch;
        // `attempt` never rejects by contract (it surfaces failures via `reportError` itself).
        await attempt(() => g.epoch === attemptEpoch);
      } while (g.dirty);
    } finally {
      g.inFlight = false;
    }
  };

  // UX-1 first-fetch flags (see the flags' doc on `AppState`): set on the FIRST settled fetch —
  // success or failure — and never reset. These helpers keep the `refresh*` bodies one-liners.
  const markFetched = (
    field:
      | "projectsFetched"
      | "ideasFetched"
      | "insightsFetched"
      | "mcpServersFetched"
      | "mcpArtifactsFetched"
      | "accountsFetched"
      | "skillsFetched",
  ): void => {
    if (!get()[field]) set({ [field]: true } as Partial<AppState>);
  };
  const markKeyedFetched = (
    field: "goalsFetched" | "tasksFetched" | "researchRunsFetched",
    key: string,
  ): void => {
    if (!get()[field][key]) set((s) => ({ [field]: { ...s[field], [key]: true } }) as Partial<AppState>);
  };

  return {
    sessions: {},
    workspaces: {},
    activeSessionId: null,
    daemonConnected: false,
    daemonIncompatible: false,
    upgradeDialogOpen: false,
    upgradeError: null,
    hydrated: false,
    view: "home",
    expanded: {},
    treeCache: {},
    selectedFile: null,
    showIgnored: false,
    filesRailOpen: false,
    watchPaused: false,
    treeEpochs: {},
    activeProjectId: null,
    projects: [],
    goalsByProject: {},
    ideas: [],
    insights: [],
    tasksByProject: {},
    researchRunsByIdea: {},
    graphByProject: {},
    rulesets: {},
    docsByProject: {},
    docViews: {},
    mcpServers: [],
    mcpToolsByServer: {},
    mcpArtifacts: [],
    accounts: [],
    skills: [],
    invocations: [],
    auditRows: [],
    policies: [],
    projectsFetched: false,
    goalsFetched: {},
    ideasFetched: false,
    insightsFetched: false,
    tasksFetched: {},
    researchRunsFetched: {},
    mcpServersFetched: false,
    mcpArtifactsFetched: false,
    accountsFetched: false,
    skillsFetched: false,
    workflows: [],
    orchdDown: false,
    orchdIncompatible: false,
    orchdUpgradeDialogOpen: false,
    storageStatus: null,
    toast: null,
    toastQueue: [],
    toastTone: "error",
    toastToneQueue: [],
    diagEvents: [],

    upsertSession: (meta) =>
      set((s) => ({ sessions: { ...s.sessions, [meta.id]: meta } })),

    removeSession: (id) =>
      set((s) => {
        if (!(id in s.sessions)) return {};
        const { [id]: _removed, ...rest } = s.sessions;
        return {
          sessions: rest,
          activeSessionId: s.activeSessionId === id ? null : s.activeSessionId,
        };
      }),

    setLifecycle: (p) =>
      set((s) => {
        const existing = s.sessions[p.sessionId];
        if (!existing) return {};
        // Exited always wins (C4): a late/out-of-order `session://state-changed` that arrives after
        // `session://exited` must not resurrect a dead session to a running lifecycle or re-set
        // waitingForInput — the same honest-state invariant markExited enforces.
        if (existing.lifecycle.kind === "exited") return {};
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              lifecycle: p.lifecycle,
              waitingForInput: p.waitingForInput,
              cwd: p.cwd,
              // The daemon only ever emits `session://state-changed` from a LIVE session's own
              // reader/ticker threads (`pty_supervisor::emit_status` is called from nowhere else,
              // and the ticker exits the moment `is_active` flips false) — so receiving one for a
              // non-exited lifecycle IS the daemon telling us the PTY is alive. Without this, a
              // session that came back `isActive:false` from a rehydrate (persistence.rs hard-codes
              // that on restore) could NEVER be shown as live again client-side, no matter what the
              // daemon reported. This is not inventing liveness: an `exited` payload is left
              // inactive, and no other path here sets `isActive` true.
              isActive: p.lifecycle.kind !== "exited",
            },
          },
        };
      }),

    markExited: (p) =>
      set((s) => {
        const existing = s.sessions[p.sessionId];
        if (!existing) return {};
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              isActive: false,
              // A finished process is never waiting for input — the honest state for every
              // consumer (stats strip, StatusDot, HomeView filters). The live `session://exited`
              // push carries no `waitingForInput` field, so a session that exited/crashed while
              // blocked on stdin would otherwise keep a stale `true` forever (review finding F1).
              waitingForInput: false,
              lifecycle: { kind: "exited", code: p.code, signal: p.signal },
            },
          },
        };
      }),

    setDaemonConnected: (connected) => set({ daemonConnected: connected }),
    setDaemonIncompatible: (v) => set({ daemonIncompatible: v }),
    // Opening the dialog fresh (v=true) clears any stale upgradeError from a previous attempt
    // (finding [13]): every reopen path (daemon://incompatible, DaemonBanner's "Update" action)
    // goes through this setter, so this is the single place that guarantees a fresh open never
    // shows a leftover error from an earlier session/attempt. Closing (v=false) leaves the error
    // untouched — Cancel doesn't need to erase it, only a fresh open does.
    setUpgradeDialogOpen: (v) => set(v ? { upgradeDialogOpen: v, upgradeError: null } : { upgradeDialogOpen: v }),
    setUpgradeError: (v) => set({ upgradeError: v }),
    setHydrated: (v) => set({ hydrated: v }),

    upsertWorkspace: (ws) =>
      set((s) => ({ workspaces: { ...s.workspaces, [ws.id]: ws } })),

    dropWorkspace: (workspaceId) =>
      set((s) => {
        const hadWorkspace = workspaceId in s.workspaces;
        const doomed = Object.values(s.sessions).filter((m) => m.workspaceId === workspaceId);
        if (!hadWorkspace && doomed.length === 0) return {}; // idempotent: nothing left to drop
        const workspaces = { ...s.workspaces };
        delete workspaces[workspaceId];
        const sessions: Record<SessionId, SessionMeta> = {};
        for (const [id, meta] of Object.entries(s.sessions)) {
          if (meta.workspaceId !== workspaceId) sessions[id] = meta;
        }
        const activeGone =
          s.activeSessionId !== null && !(s.activeSessionId in sessions);
        return {
          workspaces,
          sessions,
          activeSessionId: activeGone ? null : s.activeSessionId,
        };
      }),

    removeWorkspace: async (workspaceId) => {
      // Daemon first, state second: the workspace/sessions are dropped ONLY after the daemon has
      // actually committed the removal (SCN-058 "removal rejects → the row stays"). A rejection
      // propagates untouched so the calling surface can name the failure in its own vocabulary
      // (`WorkspaceSidebar`'s `describeCommandError` — this file's `reportError` speaks the
      // orchestrator's, which would mislabel a sessiond outage).
      await removeWorkspaceCmd(workspaceId);
      get().dropWorkspace(workspaceId);
    },

    setActiveSession: (id) => set({ activeSessionId: id }),

    setView: (v) => set({ view: v }),

    setExpanded: (root, rel, open) =>
      set((s) => {
        const key = fsKey(root, rel);
        if (open) {
          return { expanded: { ...s.expanded, [key]: true } };
        }
        if (!(key in s.expanded)) return {};
        const rest = { ...s.expanded };
        delete rest[key];
        return { expanded: rest };
      }),

    cacheDir: (root, rel, entries) =>
      set((s) => ({ treeCache: { ...s.treeCache, [fsKey(root, rel)]: entries } })),

    invalidateDirs: (root, rels) =>
      set((s) => {
        const prefix = `${root}\t`;
        const dropAll = rels.includes("*");
        const dropKeys = dropAll ? null : new Set(rels.map((rel) => fsKey(root, rel)));

        const drop = (key: string): boolean =>
          key.startsWith(prefix) && (dropAll || dropKeys!.has(key));

        // `expanded` is deliberately NOT filtered here — see the doc comment on `invalidateDirs`
        // above: this is a point refresh, not a collapse.
        const out: Record<string, FsEntry[]> = {};
        for (const key of Object.keys(s.treeCache)) {
          if (!drop(key)) out[key] = s.treeCache[key];
        }

        // FS-8: bump the invalidation epoch of every affected key, so a `listDir` response that
        // was already IN FLIGHT when this invalidation landed is recognized as stale and dropped
        // (then re-fetched) instead of silently becoming the cached truth. The bump set covers
        // keys that may have an in-flight fetch: every cached/expanded/epoch-tracked key under the
        // root for the "*" overflow sentinel, or exactly the named dirs otherwise.
        const treeEpochs = { ...s.treeEpochs };
        if (dropAll) {
          const candidates = new Set([
            ...Object.keys(treeEpochs),
            ...Object.keys(s.treeCache),
            ...Object.keys(s.expanded),
          ]);
          for (const key of candidates) {
            if (key.startsWith(prefix)) treeEpochs[key] = (treeEpochs[key] ?? 0) + 1;
          }
        } else {
          for (const key of dropKeys!) treeEpochs[key] = (treeEpochs[key] ?? 0) + 1;
        }

        return { treeCache: out, treeEpochs };
      }),

    setSelectedFile: (sel) => set({ selectedFile: sel }),

    toggleShowIgnored: () => set((s) => ({ showIgnored: !s.showIgnored })),

    setFilesRailOpen: (b) => set({ filesRailOpen: b }),

    setWatchPaused: (b) => set({ watchPaused: b }),

    showToast: (message, tone = "error") => {
      const prevHead = get().toast;
      set((s) => {
        const queue = [...s.toastQueue, message];
        const tones = [...s.toastToneQueue, tone];
        // Cap at TOAST_QUEUE_CAP, dropping the OLDEST first (BL-97, spec D8).
        while (queue.length > TOAST_QUEUE_CAP) queue.shift();
        while (tones.length > TOAST_QUEUE_CAP) tones.shift();
        return {
          toastQueue: queue,
          toastToneQueue: tones,
          toast: queue[0] ?? null,
          toastTone: tones[0] ?? "error",
        };
      });
      // (Re)start the timer only when the VISIBLE toast actually changed — either the queue was
      // empty (a first toast appeared) or drop-oldest bumped the head. A steady head keeps its
      // original deadline, so a burst of queued toasts never resets the visible one's clock.
      if (get().toast !== prevHead) armToastTimer();
    },

    dismissToast: () => {
      set((s) => {
        const queue = s.toastQueue.slice(1);
        const tones = s.toastToneQueue.slice(1);
        return {
          toastQueue: queue,
          toastToneQueue: tones,
          toast: queue[0] ?? null,
          toastTone: tones[0] ?? "error",
        };
      });
      // Re-arm for the newly-visible toast, or cancel outright when the queue drained — so a stale
      // timer can never clear a toast shown after this one (the honest-close invariant).
      if (get().toast !== null) armToastTimer();
      else clearToastTimer();
    },

    reportError: (op, e) => {
      // Scrub the human message too, not just `detail` (C1): describeOrchdError passes an
      // unmapped/Io daemon message through verbatim, so a raw `/Users/<name>` path or an embedded
      // key would otherwise reach the toast, the console, and — the real leak — the copyable
      // support bundle (toSupportBundle assumes every stored event is already scrubbed, as
      // recordRenderCrash's message is).
      //
      // FE-2: a STRING error is already a finished human message (e.g. a per-source failure reason
      // like "scan worker died: …" handed over by `refreshStats`) — pass it through verbatim.
      // Running it through `describeOrchdError` would mislabel every such failure "unknown
      // orchestrator error", hiding the actual cause from the one surface meant to show it.
      const message = scrubSecrets(
        typeof e === "string" && e ? e : describeOrchdError(e),
      );
      const { kind, detail } = classifyError(e);
      const event: DiagEvent = {
        id: ++diagSeq,
        ts: Date.now(),
        op,
        kind,
        message,
        detail: detail ? scrubSecrets(detail) : null,
      };
      set((s) => ({ diagEvents: pushCapped(s.diagEvents, event, DIAG_CAP) }));
      // A structured, secret-scrubbed devtools breadcrumb IN ADDITION to the ring + toast — so a
      // failure is reconstructable from the console too, never console-only (spec §7).
      console.error(`[diag] ${op} (${kind}): ${message}`, event.detail ?? "");
      get().showToast(message);
      return message;
    },
    clearDiag: () => set({ diagEvents: [] }),

    recordRenderCrash: (error, componentStack) => {
      const event: DiagEvent = {
        id: ++diagSeq,
        ts: Date.now(),
        op: "render",
        kind: "render",
        message: scrubSecrets(error.message || "render error"),
        detail: componentStack ? scrubSecrets(componentStack.trim().split("\n").slice(0, 6).join("\n")) : null,
      };
      set((s) => ({ diagEvents: pushCapped(s.diagEvents, event, DIAG_CAP) }));
      console.error(`[diag] render crash: ${event.message}`, event.detail ?? "");
    },

    // ── app-domain slice (spec §10, S3 T13) ─────────────────────────────────────────────────
    //
    // Every `refresh*` below follows the same shape: `guardedRefresh` (FE-1 race guard — see its
    // comment above) around fetch via `./orchd.ts`, replace the matching slice on success, or
    // surface the mapped honest message as a toast on failure (spec §7 "every async failure is a
    // toast... never console-only") — never a silent no-op, never a thrown/unhandled rejection.
    // The UX-1 first-fetch flag flips on the first SETTLED attempt (success or failure), so a
    // list surface can tell initial-loading apart from genuinely-empty.

    refreshProjects: async () => {
      await guardedRefresh("projects", async (isCurrent) => {
        try {
          const projects = await orchdListProjects();
          if (!isCurrent()) return; // superseded mid-flight — the trailing re-run applies fresher data
          set({ projects });
          markFetched("projectsFetched");
        } catch (e) {
          if (isCurrent()) markFetched("projectsFetched");
          get().reportError("refreshProjects", e);
        }
      });
    },

    refreshGoals: async (projectId) => {
      await guardedRefresh(`goals:${projectId}`, async (isCurrent) => {
        try {
          const goals = await orchdListGoals(projectId);
          if (!isCurrent()) return;
          set((s) => ({ goalsByProject: { ...s.goalsByProject, [projectId]: goals } }));
          markKeyedFetched("goalsFetched", projectId);
        } catch (e) {
          if (isCurrent()) markKeyedFetched("goalsFetched", projectId);
          get().reportError("refreshGoals", e);
        }
      });
    },

    refreshIdeas: async () => {
      await guardedRefresh("ideas", async (isCurrent) => {
        try {
          const ideas = await orchdListIdeas(null);
          if (!isCurrent()) return;
          set({ ideas });
          markFetched("ideasFetched");
        } catch (e) {
          if (isCurrent()) markFetched("ideasFetched");
          get().reportError("refreshIdeas", e);
        }
      });
    },

    refreshInsights: async () => {
      await guardedRefresh("insights", async (isCurrent) => {
        try {
          const insights = await orchdListInsights(null);
          if (!isCurrent()) return;
          set({ insights });
          markFetched("insightsFetched");
        } catch (e) {
          if (isCurrent()) markFetched("insightsFetched");
          get().reportError("refreshInsights", e);
        }
      });
    },

    refreshTasks: async (projectId) => {
      await guardedRefresh(`tasks:${projectId}`, async (isCurrent) => {
        try {
          const tasks = await orchdListTasks(projectId);
          if (!isCurrent()) return;
          set((s) => ({ tasksByProject: { ...s.tasksByProject, [projectId]: tasks } }));
          markKeyedFetched("tasksFetched", projectId);
        } catch (e) {
          if (isCurrent()) markKeyedFetched("tasksFetched", projectId);
          get().reportError("refreshTasks", e);
        }
      });
    },

    refreshResearchRuns: async (ideaId) => {
      await guardedRefresh(`researchRuns:${ideaId}`, async (isCurrent) => {
        try {
          const runs = await researchListRuns(ideaId);
          if (!isCurrent()) return;
          set((s) => ({ researchRunsByIdea: { ...s.researchRunsByIdea, [ideaId]: runs } }));
          markKeyedFetched("researchRunsFetched", ideaId);
        } catch (e) {
          if (isCurrent()) markKeyedFetched("researchRunsFetched", ideaId);
          get().reportError("refreshResearchRuns", e);
        }
      });
    },

    refreshGraph: async (projectId) => {
      await guardedRefresh(`graph:${projectId}`, async (isCurrent) => {
        try {
          const graph = await orchdGraphListProject(projectId);
          if (!isCurrent()) return;
          set((s) => ({ graphByProject: { ...s.graphByProject, [projectId]: graph } }));
        } catch (e) {
          get().reportError("refreshGraph", e);
        }
      });
    },

    refreshRuleset: async (key) => {
      const { scope, projectId } = parseRulesetKey(key);
      await guardedRefresh(`ruleset:${key}`, async (isCurrent) => {
        try {
          const view = await orchdGetRuleset(scope, projectId);
          if (!isCurrent()) return;
          set((s) => ({ rulesets: { ...s.rulesets, [key]: view } }));
        } catch (e) {
          get().reportError("refreshRuleset", e);
        }
      });
    },

    // ── SCN-054 project docs ─────────────────────────────────────────────────────────────────

    refreshDocs: async (projectId) => {
      await guardedRefresh(`docs:${projectId}`, async (isCurrent) => {
        try {
          const docs = await orchdListDocs(projectId);
          if (!isCurrent()) return;
          set((s) => ({ docsByProject: { ...s.docsByProject, [projectId]: docs } }));
        } catch (e) {
          get().reportError("refreshDocs", e);
        }
      });
    },

    refreshDoc: async (projectId, name) => {
      const key = docViewKey(projectId, name);
      await guardedRefresh(`doc:${key}`, async (isCurrent) => {
        try {
          const view = await orchdGetDoc(projectId, name);
          if (!isCurrent()) return;
          set((s) => ({ docViews: { ...s.docViews, [key]: view } }));
        } catch (e) {
          if (isNotFoundError(e)) {
            // The doc was deleted (by this client's own confirmed delete, or another client's,
            // racing this refresh) — dropping the stale view IS the correct outcome, not an error
            // (see `refreshDoc`'s interface doc above).
            if (!isCurrent()) return;
            set((s) => {
              const { [key]: _dropped, ...rest } = s.docViews;
              return { docViews: rest };
            });
            return;
          }
          get().reportError("refreshDoc", e);
        }
      });
    },

    openProject: (id) => set({ view: "project", activeProjectId: id }),

    // ── MCP slice (S-EXT §8, T8) ─────────────────────────────────────────────────────────────

    refreshMcpServers: async () => {
      await guardedRefresh("mcpServers", async (isCurrent) => {
        try {
          const mcpServers = await mcpListServers(null);
          if (!isCurrent()) return;
          set({ mcpServers });
          markFetched("mcpServersFetched");
        } catch (e) {
          if (isCurrent()) markFetched("mcpServersFetched");
          get().reportError("refreshMcpServers", e);
        }
      });
    },

    refreshMcpTools: async (serverId) => {
      await guardedRefresh(`mcpTools:${serverId}`, async (isCurrent) => {
        try {
          const tools = await mcpListTools(serverId);
          if (!isCurrent()) return;
          set((s) => ({ mcpToolsByServer: { ...s.mcpToolsByServer, [serverId]: tools } }));
        } catch (e) {
          get().reportError("refreshMcpTools", e);
        }
      });
    },

    refreshMcpArtifacts: async () => {
      await guardedRefresh("mcpArtifacts", async (isCurrent) => {
        try {
          const mcpArtifacts = await mcpListArtifacts(null, null, null);
          if (!isCurrent()) return;
          set({ mcpArtifacts });
          markFetched("mcpArtifactsFetched");
        } catch (e) {
          if (isCurrent()) markFetched("mcpArtifactsFetched");
          get().reportError("refreshMcpArtifacts", e);
        }
      });
    },

    // ── Connectors slice (S-EXT §8, T13b) ────────────────────────────────────────────────────

    refreshAccounts: async () => {
      await guardedRefresh("accounts", async (isCurrent) => {
        try {
          const accounts = await connectorListAccounts();
          if (!isCurrent()) return;
          set({ accounts });
        } catch (e) {
          get().reportError("refreshAccounts", e);
        } finally {
          markFetched("accountsFetched"); // BL-194 (UX-1): no false "No accounts" flash
        }
      });
    },

    // ── Skills slice (S-EXT §8, D11, Q14, T17) ───────────────────────────────────────────────

    refreshSkills: async () => {
      await guardedRefresh("skills", async (isCurrent) => {
        try {
          const skills = await skillList(null);
          if (!isCurrent()) return;
          set({ skills });
        } catch (e) {
          get().reportError("refreshSkills", e);
        } finally {
          markFetched("skillsFetched"); // BL-194 (UX-1): no false "No skills" flash
        }
      });
    },

    // ── Trust slice (S-EXT §4/§6/§8, BL-22, T18) ─────────────────────────────────────────────

    refreshInvocations: () => {
      // Trailing 300ms debounce ON TOP of the usual race guard (FE-1): `App.tsx` fires this on
      // EVERY `orchd://mcp-invocation-logged` push, i.e. potentially per MCP call — a busy server
      // would otherwise keep one `mcp_list_invocations` round-trip in flight at all times. Calls
      // inside the window are collapsed into a single trailing fetch; the returned promise
      // resolves once that fetch (and any guard-mandated re-run) completes, so direct awaiters
      // (mount/refresh paths) observe the same completion semantics as any other `refresh*`.
      return new Promise<void>((resolve) => {
        invocationsWaiters.push(resolve);
        if (invocationsTimer !== undefined) clearTimeout(invocationsTimer);
        invocationsTimer = setTimeout(() => {
          invocationsTimer = undefined;
          const waiters = invocationsWaiters;
          invocationsWaiters = [];
          void guardedRefresh("invocations", async (isCurrent) => {
            try {
              const invocations = await mcpListInvocations(null, null, null);
              if (!isCurrent()) return;
              set({ invocations });
            } catch (e) {
              get().reportError("refreshInvocations", e);
            }
          }).then(() => {
            for (const done of waiters) done();
          });
        }, INVOCATIONS_DEBOUNCE_MS);
      });
    },

    refreshAuditRows: async () => {
      await guardedRefresh("auditRows", async (isCurrent) => {
        try {
          const auditRows = await trustListAudit(null);
          if (!isCurrent()) return;
          set({ auditRows });
        } catch (e) {
          get().reportError("refreshAuditRows", e);
        }
      });
    },

    refreshPolicies: async () => {
      await guardedRefresh("policies", async (isCurrent) => {
        try {
          const policies = await trustListPolicies();
          if (!isCurrent()) return;
          set({ policies });
        } catch (e) {
          get().reportError("refreshPolicies", e);
        }
      });
    },

    // ── Workflow-authoring slice (SW1) ───────────────────────────────────────────────────────

    refreshWorkflows: async () => {
      await guardedRefresh("workflows", async (isCurrent) => {
        try {
          const workflows = await orchdListWorkflows(null, null);
          if (!isCurrent()) return;
          set({ workflows });
        } catch (e) {
          get().reportError("refreshWorkflows", e);
        }
      });
    },

    upsertWorkflow: async (params) => {
      // Daemon first: the write is authoritative (fail-closed validation runs there). On success
      // upsert the returned row by id so the library reflects it immediately even before the
      // `orchd://workflows-changed` push re-fetches wholesale; a rejection propagates untouched so
      // the editor keeps the draft and toasts the honest message itself (mirrors `removeWorkspace`'s
      // daemon-first, propagate-on-reject discipline).
      const saved = await orchdUpsertWorkflow(
        params.id,
        params.name,
        params.description,
        params.scope,
        params.projectId,
        params.defaultAgent,
        params.stages,
        params.globalSkillIds,
        params.supervisor,
      );
      set((s) => {
        const rest = s.workflows.filter((w) => w.id !== saved.id);
        return { workflows: [...rest, saved] };
      });
      return saved;
    },

    deleteWorkflow: async (id) => {
      // Daemon first, state second (mirrors `removeWorkspace`): the row is dropped ONLY after the
      // daemon commits the delete; a rejection leaves `workflows` exactly as it was so the row stays
      // put and the library surfaces the honest toast.
      await orchdDeleteWorkflow(id);
      set((s) => ({ workflows: s.workflows.filter((w) => w.id !== id) }));
    },

    // ── storage-degradation status (spec D3, BL-94) ──────────────────────────────────────────

    refreshStorageStatus: async () => {
      await guardedRefresh("storageStatus", async (isCurrent) => {
        try {
          const storageStatus = await orchdStorageStatus();
          if (!isCurrent()) return;
          set({ storageStatus });
        } catch (e) {
          get().reportError("refreshStorageStatus", e);
        }
      });
    },

    setOrchdDown: (v) => set({ orchdDown: v }),

    theme: readTheme(),
    setTheme: (t) => {
      setThemePref(t);
      set({ theme: t });
    },
    setOrchdIncompatible: (v) => set({ orchdIncompatible: v }),
    setOrchdUpgradeDialogOpen: (v) => set({ orchdUpgradeDialogOpen: v }),

    // ── keep-awake (SCN-045 / FLW-18) ────────────────────────────────────────────────────────
    //
    // `active`/`error` ALWAYS come from the core's reconciled `PowerStatus` (the assertion's
    // real held-state), never from the toggle intent — the pill can therefore never claim an
    // "awake" the OS denied (spec §7 / SCN-045 honesty). Failures follow the `refresh*` shape:
    // caught here, surfaced via the store's one reporting path, never an unhandled rejection.

    keepAwake: { enabled: readKeepAwakeEnabled(), active: false, error: null },

    setKeepAwakeEnabled: async (enabled) => {
      // Persist + optimistically flip the toggle so the pill answers the click immediately;
      // `active` deliberately stays as-is until the core's reconcile below answers.
      persistKeepAwakeEnabled(enabled);
      set((s) => ({ keepAwake: { ...s.keepAwake, enabled } }));
      try {
        applyPowerStatus("setKeepAwakeEnabled", await powerSetEnabled(enabled));
      } catch (e) {
        // The invoke itself broke (the command is infallible at the wire layer): keep the
        // persisted intent, but the HOLD state must degrade honestly — never claim active.
        const reason = describePowerFailure(e);
        set((s) => ({ keepAwake: { ...s.keepAwake, active: false, error: reason } }));
        reportPowerFailure("setKeepAwakeEnabled", reason);
      }
    },

    syncKeepAwake: async (liveCount) => {
      try {
        applyPowerStatus("syncKeepAwake", await powerSyncSessions(liveCount));
      } catch (e) {
        const reason = describePowerFailure(e);
        set((s) => ({ keepAwake: { ...s.keepAwake, active: false, error: reason } }));
        reportPowerFailure("syncKeepAwake", reason);
      }
    },

    // ── stats slice (SCN-052/053) ───────────────────────────────────────────────────────────────

    stats: {
      range: "30d",
      usage: null,
      git: null,
      usageError: null,
      gitError: null,
      loading: false,
      epoch: 0,
      lastRefreshMs: null,
    },

    setStatsRange: async (range) => {
      set((s) => ({ stats: { ...s.stats, range } }));
      await get().refreshStats();
    },

    cancelStats: () => {
      // Bump the epoch so any in-flight reply is discarded on arrival, and drop the spinner.
      set((s) => ({ stats: { ...s.stats, epoch: s.stats.epoch + 1, loading: false } }));
    },

    refreshStats: async () => {
      // Both sources fetch in parallel and fail INDEPENDENTLY (SCN-052 per-source honesty):
      // a usage-scan failure must never blank the git section, and vice versa. Failures land
      // in the slice (the view's inline notes) AND in Diagnostics via `reportError` — same
      // one-path rule every other refresh follows.
      const { range } = get().stats;
      const roots = Object.values(get().workspaces).flatMap((w) => w.roots ?? [w.rootPath]);
      // Claim an epoch for THIS scan. A range switch, a Refresh, or a Cancel mid-flight bumps it;
      // when our two calls settle we only apply if the epoch is still ours — otherwise a slower,
      // stale scan would overwrite the newer range's figures under the wrong label (AUD-…-25).
      const myEpoch = get().stats.epoch + 1;
      set((s) => ({ stats: { ...s.stats, epoch: myEpoch, loading: true } }));
      const [usageRes, gitRes] = await Promise.allSettled([
        statsUsage(range),
        statsGit(roots, range),
      ]);
      if (get().stats.epoch !== myEpoch) return; // superseded or cancelled — discard silently
      set((s) => {
        const next = { ...s.stats, loading: false, lastRefreshMs: Date.now() };
        if (usageRes.status === "fulfilled") {
          next.usage = usageRes.value;
          next.usageError = usageRes.value.error;
        } else {
          next.usage = null;
          next.usageError =
            usageRes.reason instanceof Error ? usageRes.reason.message : String(usageRes.reason);
        }
        if (gitRes.status === "fulfilled") {
          next.git = gitRes.value;
          next.gitError = null;
        } else {
          next.git = null;
          next.gitError =
            gitRes.reason instanceof Error ? gitRes.reason.message : String(gitRes.reason);
        }
        return { stats: next };
      });
      const after = get().stats;
      if (after.usageError) get().reportError("refreshStats", after.usageError);
      if (after.gitError) get().reportError("refreshStats", after.gitError);
    },
  };
});
