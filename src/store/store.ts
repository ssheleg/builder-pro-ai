import { create } from "zustand";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";
import type { FsEntry } from "../ipc/fs";
import type {
  Account,
  AuditRow,
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
} from "../ipc/orchd-types";
import {
  orchdGetRuleset,
  orchdGraphListProject,
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
  describeOrchdError,
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
   * S-EXT «Расширения» panel (`ExtPanel`, T8) — MCP servers/tools/connectors/skills management.
   * Defaults to `"home"` — the owner's daily loop starts there, never mid-workspace.
   */
  view: "home" | "workspace" | "project" | "ext";

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
   * Queue-of-ONE toast message (design-system.md Toast atom, spec §7 "honest error surface" —
   * every async failure is a toast with the mapped human message, never console-only). `null`
   * means no toast is showing. `showToast` REPLACES whatever is currently shown — there is no
   * queue behind it, matching the design-system's "one inbox" spirit applied to transient
   * notices: at most one thing asks for the owner's attention via a toast at a time.
   */
  toast: string | null;

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
   * ⌘K quick-capture inbox and Идеи panel filter client-side). Replaced wholesale by
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

  /**
   * MCP slice (S-EXT §8, T8: the «Расширения» view's Servers/Tools tabs). Mirrors the app-domain
   * slice above exactly — invalidation-driven (D6: coarse `orchd://mcp-*-changed` pushes tell the
   * frontend WHAT changed; the matching `refresh*` action re-fetches wholesale/per-key from
   * `./orchd.ts`, replacing it — no client-side merge/patch).
   */
  /** Every MCP server (global scope — `mcpListServers(null)`; Phase 1's «Расширения» view has no
   * per-project server list yet). Replaced wholesale by `refreshMcpServers`, mirrors `projects`. */
  mcpServers: McpServer[];
  /** A server's cached tool list (from `mcp_tool`, refreshed on connect/`list_changed`), keyed by
   * `serverId`. Absence means "not yet fetched" — same convention as `goalsByProject`/
   * `tasksByProject`. Replaced per-key by `refreshMcpTools(serverId)`; a `McpToolsChanged
   * {serverId}` push never touches any OTHER server's entry. */
  mcpToolsByServer: Record<string, McpTool[]>;
  /** Every durable MCP artifact, unfiltered (mirrors `ideas`/`insights`'s whole-store
   * convention — the Артефакты tab filters client-side). Replaced wholesale by
   * `refreshMcpArtifacts`. */
  mcpArtifacts: McpArtifact[];
  /** Every connector account (S-EXT §8, T13b: the «Расширения»/«Коннекторы» tab). Mirrors
   * `mcpServers`'s whole-store, un-scoped convention exactly (`connectorListAccounts` has no
   * filter either) — replaced wholesale by `refreshAccounts`. */
  accounts: Account[];
  /** Every skill (global scope — `skillList(null)`; S-EXT §8, D11, T17: the «Расширения»/
   * «Навыки» tab. PLUMBING ONLY — no runtime consumer until S6b). Mirrors `mcpServers`'s
   * whole-store, un-scoped convention exactly — replaced wholesale by `refreshSkills`. */
  skills: Skill[];
  /** Every MCP invocation, unfiltered (S-EXT §8, T18: the «Расширения»/«Журнал» tab). Mirrors
   * `mcpArtifacts`'s whole-store, un-scoped convention exactly — replaced wholesale by
   * `refreshInvocations`. */
  invocations: McpInvocation[];
  /** Every `audit_log` row, newest-first (S-EXT §4/§6/§8, BL-22, T18: the «Расширения»/
   * «Журнал» tab's audit view). Replaced wholesale by `refreshAuditRows`. */
  auditRows: AuditRow[];
  /** Every configured spend/rate policy (S-EXT §4/§6/§8, BL-22, T18: the «Расширения»/«Журнал»
   * tab's policy editor). Replaced wholesale by `refreshPolicies`. */
  policies: Policy[];

  /** Honest orchd connectivity (spec §9/§11, mirrors sessiond's `daemonConnected` inverted):
   * `true` while the `orchd://down` event is the most recent connection-state signal seen, `false`
   * once `orchd://up` fires. Every domain surface shows the shared "Оркестратор недоступен"
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
  setActiveSession: (id: SessionId | null) => void;

  /** Switch the top-level view. See `view`'s doc above. */
  setView: (v: "home" | "workspace" | "project" | "ext") => void;
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
   * Show a toast (replacing any current one) and auto-dismiss it after `TOAST_AUTO_DISMISS_MS`.
   * See `toast`'s doc above. `<Toast/>` (`src/components/Toast.tsx`) is a pure reader of `toast`
   * — it never owns this timer itself, so the auto-dismiss fires even across a remount.
   */
  showToast: (message: string) => void;
  /** Clear the current toast immediately (e.g. a manual dismiss action) and cancel its pending
   * auto-dismiss timer so it cannot later clear a DIFFERENT toast shown after this one. */
  dismissToast: () => void;

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
   * Mirrors `refreshMcpArtifacts` exactly. */
  refreshInvocations: () => Promise<void>;
  /** Re-fetch `auditRows` wholesale (`trustListAudit(null)` — no cap). Mirrors
   * `refreshInvocations` exactly. */
  refreshAuditRows: () => Promise<void>;
  /** Re-fetch `policies` wholesale (`trustListPolicies()` has no filter). Mirrors
   * `refreshMcpServers` exactly. */
  refreshPolicies: () => Promise<void>;
  /** Set `orchdDown`. See its doc above. */
  setOrchdDown: (v: boolean) => void;
  /** Set `orchdIncompatible`. See its doc above — never auto-clears, mirrors
   * `setDaemonIncompatible`. */
  setOrchdIncompatible: (v: boolean) => void;
  /** Set `orchdUpgradeDialogOpen`. See its doc above. */
  setOrchdUpgradeDialogOpen: (v: boolean) => void;
}

/** Key format shared by `expanded`/`treeCache` — see their docs on `AppState` above. */
function fsKey(root: string, rel: string): string {
  return `${root}\t${rel}`;
}

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

/** How long a toast stays up before auto-dismissing (Toast atom, spec §7). */
const TOAST_AUTO_DISMISS_MS = 4000;

export const useAppStore = create<AppState>((set, get) => {
  // Toast auto-dismiss bookkeeping (closure state, not store state — it's write-only plumbing,
  // like terminal-manager's attachGeneration guard). `token` is bumped by every showToast/
  // dismissToast call; a pending timeout only clears the toast if its OWN token still matches the
  // current one, so an earlier toast's timer can never clear a later, different toast (it always
  // can't anyway, since we clearTimeout the previous timer below — the token is defense in depth
  // matching the rest of this codebase's race-guard style).
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let toastToken = 0;

  const clearToastTimer = (): void => {
    if (toastTimer !== undefined) {
      clearTimeout(toastTimer);
      toastTimer = undefined;
    }
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
    activeProjectId: null,
    projects: [],
    goalsByProject: {},
    ideas: [],
    insights: [],
    tasksByProject: {},
    researchRunsByIdea: {},
    graphByProject: {},
    rulesets: {},
    mcpServers: [],
    mcpToolsByServer: {},
    mcpArtifacts: [],
    accounts: [],
    skills: [],
    invocations: [],
    auditRows: [],
    policies: [],
    orchdDown: false,
    orchdIncompatible: false,
    orchdUpgradeDialogOpen: false,
    toast: null,

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
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              lifecycle: p.lifecycle,
              waitingForInput: p.waitingForInput,
              cwd: p.cwd,
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
    // (finding [13]): every reopen path (daemon://incompatible, DaemonBanner's "Обновить" action)
    // goes through this setter, so this is the single place that guarantees a fresh open never
    // shows a leftover error from an earlier session/attempt. Closing (v=false) leaves the error
    // untouched — Cancel doesn't need to erase it, only a fresh open does.
    setUpgradeDialogOpen: (v) => set(v ? { upgradeDialogOpen: v, upgradeError: null } : { upgradeDialogOpen: v }),
    setUpgradeError: (v) => set({ upgradeError: v }),
    setHydrated: (v) => set({ hydrated: v }),

    upsertWorkspace: (ws) =>
      set((s) => ({ workspaces: { ...s.workspaces, [ws.id]: ws } })),

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

        return { treeCache: out };
      }),

    setSelectedFile: (sel) => set({ selectedFile: sel }),

    toggleShowIgnored: () => set((s) => ({ showIgnored: !s.showIgnored })),

    setFilesRailOpen: (b) => set({ filesRailOpen: b }),

    setWatchPaused: (b) => set({ watchPaused: b }),

    showToast: (message) => {
      clearToastTimer();
      const token = ++toastToken;
      set({ toast: message });
      toastTimer = setTimeout(() => {
        // Only this call's own token still being current proves no later showToast/dismissToast
        // has superseded it — otherwise this stale timer must not touch whatever toast is showing
        // now (see the doc comment above `toastTimer`).
        if (token === toastToken) set({ toast: null });
        toastTimer = undefined;
      }, TOAST_AUTO_DISMISS_MS);
    },

    dismissToast: () => {
      clearToastTimer();
      toastToken += 1;
      set({ toast: null });
    },

    // ── app-domain slice (spec §10, S3 T13) ─────────────────────────────────────────────────
    //
    // Every `refresh*` below follows the same shape: fetch via `./orchd.ts`, replace the
    // matching slice on success, or surface the mapped honest message as a toast on failure
    // (spec §7 "every async failure is a toast... never console-only") — never a silent no-op,
    // never a thrown/unhandled rejection.

    refreshProjects: async () => {
      try {
        const projects = await orchdListProjects();
        set({ projects });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshGoals: async (projectId) => {
      try {
        const goals = await orchdListGoals(projectId);
        set((s) => ({ goalsByProject: { ...s.goalsByProject, [projectId]: goals } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshIdeas: async () => {
      try {
        const ideas = await orchdListIdeas(null);
        set({ ideas });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshInsights: async () => {
      try {
        const insights = await orchdListInsights(null);
        set({ insights });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshTasks: async (projectId) => {
      try {
        const tasks = await orchdListTasks(projectId);
        set((s) => ({ tasksByProject: { ...s.tasksByProject, [projectId]: tasks } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshResearchRuns: async (ideaId) => {
      try {
        const runs = await researchListRuns(ideaId);
        set((s) => ({ researchRunsByIdea: { ...s.researchRunsByIdea, [ideaId]: runs } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshGraph: async (projectId) => {
      try {
        const graph = await orchdGraphListProject(projectId);
        set((s) => ({ graphByProject: { ...s.graphByProject, [projectId]: graph } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshRuleset: async (key) => {
      const { scope, projectId } = parseRulesetKey(key);
      try {
        const view = await orchdGetRuleset(scope, projectId);
        set((s) => ({ rulesets: { ...s.rulesets, [key]: view } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    openProject: (id) => set({ view: "project", activeProjectId: id }),

    // ── MCP slice (S-EXT §8, T8) ─────────────────────────────────────────────────────────────

    refreshMcpServers: async () => {
      try {
        const mcpServers = await mcpListServers(null);
        set({ mcpServers });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshMcpTools: async (serverId) => {
      try {
        const tools = await mcpListTools(serverId);
        set((s) => ({ mcpToolsByServer: { ...s.mcpToolsByServer, [serverId]: tools } }));
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshMcpArtifacts: async () => {
      try {
        const mcpArtifacts = await mcpListArtifacts(null, null, null);
        set({ mcpArtifacts });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    // ── Connectors slice (S-EXT §8, T13b) ────────────────────────────────────────────────────

    refreshAccounts: async () => {
      try {
        const accounts = await connectorListAccounts();
        set({ accounts });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    // ── Skills slice (S-EXT §8, D11, Q14, T17) ───────────────────────────────────────────────

    refreshSkills: async () => {
      try {
        const skills = await skillList(null);
        set({ skills });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    // ── Trust slice (S-EXT §4/§6/§8, BL-22, T18) ─────────────────────────────────────────────

    refreshInvocations: async () => {
      try {
        const invocations = await mcpListInvocations(null, null, null);
        set({ invocations });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshAuditRows: async () => {
      try {
        const auditRows = await trustListAudit(null);
        set({ auditRows });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    refreshPolicies: async () => {
      try {
        const policies = await trustListPolicies();
        set({ policies });
      } catch (e) {
        get().showToast(describeOrchdError(e));
      }
    },

    setOrchdDown: (v) => set({ orchdDown: v }),
    setOrchdIncompatible: (v) => set({ orchdIncompatible: v }),
    setOrchdUpgradeDialogOpen: (v) => set({ orchdUpgradeDialogOpen: v }),
  };
});
