import { invoke } from "@tauri-apps/api/core";
import { strings } from "../strings";
import type { WorkspaceId } from "./commands";
import type {
  Account,
  AuditRow,
  ConnectorOp,
  DocMeta,
  DocView,
  DomainTask,
  FitVerdict,
  Goal,
  GoalKind,
  GoalStatus,
  GraphEdge,
  GraphEdgeKind,
  GraphNeighborhood,
  GraphNode,
  GraphNodeKind,
  GraphView,
  Idea,
  IdeaLifecycle,
  Insight,
  InsightStatus,
  McpArtifact,
  McpAuthKind,
  McpCallResult,
  McpConnectReport,
  McpInvocation,
  McpScope,
  McpServer,
  McpTool,
  McpTransport,
  OAuthChallenge,
  Policy,
  PolicyRules,
  PolicyScope,
  Project,
  ResearchRun,
  RuleScope,
  RuleSetView,
  Skill,
  SkillScope,
  StorageStatus,
  TaskPriority,
  TaskSource,
  TaskStatus,
} from "./orchd-types";

/**
 * `orchd_import_bundle`/`orchd_import_from_file`'s success shape (spec §8/§9,
 * `src-tauri/src/commands.rs::ImportReport`, `#[serde(rename_all = "camelCase")]`). Per-family row
 * counts — NOT ts-rs generated (it is a `#[tauri::command]`-only return shape, not one of
 * `bpa_orchd_proto`'s wire types), same rationale as `commands.ts`'s hand-written `DaemonStatus`.
 */
export interface ImportReport {
  projects: number;
  goals: number;
  ideas: number;
  insights: number;
  tasks: number;
  rulesets: number;
}

/**
 * Typed `invoke()` wrappers for the `orchd_*` `#[tauri::command]` surface (spec §9/§10, S3 T13).
 * One wrapper per command, named + argument-shaped VERBATIM after
 * `src-tauri/src/commands.rs`'s `orchd_*` functions (Tauri maps each JS camelCase arg key to its
 * Rust snake_case parameter automatically, same convention as `./commands.ts`). `Option<T>`
 * parameters are modeled as `T | null` (never `undefined`/optional) — matching how ts-rs itself
 * models `Option<T>` in `./orchd-types.ts` and D11's "absent/null = unchanged" wire semantics for
 * update verbs.
 *
 * Rejections propagate as-is: a rejected `CommandError` from `commands.rs` (see
 * `describeOrchdError` below for turning one into a human message). Handling/gating those
 * rejections is the caller's job (the store's `refresh*` actions, `store.ts`), not this layer's.
 */

export function orchdPing(): Promise<void> {
  return invoke<void>("orchd_ping");
}

// ── projects ─────────────────────────────────────────────────────────────────────────────────

export function orchdCreateProject(
  name: string,
  description: string,
  workspaceIds: WorkspaceId[],
): Promise<Project> {
  return invoke<Project>("orchd_create_project", { name, description, workspaceIds });
}

export function orchdUpdateProject(
  id: string,
  name: string | null,
  description: string | null,
): Promise<Project> {
  return invoke<Project>("orchd_update_project", { id, name, description });
}

export function orchdArchiveProject(id: string): Promise<Project> {
  return invoke<Project>("orchd_archive_project", { id });
}

export function orchdUnarchiveProject(id: string): Promise<Project> {
  return invoke<Project>("orchd_unarchive_project", { id });
}

export function orchdListProjects(): Promise<Project[]> {
  return invoke<Project[]>("orchd_list_projects");
}

export function orchdAddProjectWorkspace(
  projectId: string,
  workspaceId: WorkspaceId,
): Promise<Project> {
  return invoke<Project>("orchd_add_project_workspace", { projectId, workspaceId });
}

export function orchdRemoveProjectWorkspace(
  projectId: string,
  workspaceId: WorkspaceId,
): Promise<Project> {
  return invoke<Project>("orchd_remove_project_workspace", { projectId, workspaceId });
}

// ── goals ────────────────────────────────────────────────────────────────────────────────────

export function orchdCreateGoal(
  projectId: string,
  parentId: string | null,
  kind: GoalKind,
  title: string,
  body: string,
): Promise<Goal> {
  return invoke<Goal>("orchd_create_goal", { projectId, parentId, kind, title, body });
}

export function orchdUpdateGoal(
  id: string,
  title: string | null,
  body: string | null,
  status: GoalStatus | null,
  metricRefs: string[] | null,
): Promise<Goal> {
  return invoke<Goal>("orchd_update_goal", { id, title, body, status, metricRefs });
}

export function orchdMoveGoal(
  id: string,
  newParentId: string | null,
  newOrd: number,
): Promise<Goal> {
  return invoke<Goal>("orchd_move_goal", { id, newParentId, newOrd });
}

export function orchdDeleteGoal(id: string): Promise<void> {
  return invoke<void>("orchd_delete_goal", { id });
}

export function orchdListGoals(projectId: string): Promise<Goal[]> {
  return invoke<Goal[]>("orchd_list_goals", { projectId });
}

// ── ideas ────────────────────────────────────────────────────────────────────────────────────

export function orchdCreateIdea(
  projectId: string | null,
  title: string,
  body: string,
): Promise<Idea> {
  return invoke<Idea>("orchd_create_idea", { projectId, title, body });
}

export function orchdUpdateIdea(
  id: string,
  title: string | null,
  body: string | null,
): Promise<Idea> {
  return invoke<Idea>("orchd_update_idea", { id, title, body });
}

export function orchdSetIdeaProject(id: string, projectId: string | null): Promise<Idea> {
  return invoke<Idea>("orchd_set_idea_project", { id, projectId });
}

export function orchdSetIdeaLifecycle(id: string, lifecycle: IdeaLifecycle): Promise<Idea> {
  return invoke<Idea>("orchd_set_idea_lifecycle", { id, lifecycle });
}

export function orchdDeleteIdea(id: string): Promise<void> {
  return invoke<void>("orchd_delete_idea", { id });
}

export function orchdListIdeas(projectId: string | null): Promise<Idea[]> {
  return invoke<Idea[]>("orchd_list_ideas", { projectId });
}

// ── insights ─────────────────────────────────────────────────────────────────────────────────

export function orchdCreateInsight(
  projectId: string | null,
  source: string,
  title: string,
  body: string,
): Promise<Insight> {
  return invoke<Insight>("orchd_create_insight", { projectId, source, title, body });
}

export function orchdUpdateInsight(
  id: string,
  title: string | null,
  body: string | null,
): Promise<Insight> {
  return invoke<Insight>("orchd_update_insight", { id, title, body });
}

export function orchdSetInsightFitVerdict(
  id: string,
  fitVerdict: FitVerdict | null,
  fitReasoning: string,
): Promise<Insight> {
  return invoke<Insight>("orchd_set_insight_fit_verdict", { id, fitVerdict, fitReasoning });
}

export function orchdSetInsightStatus(
  id: string,
  status: InsightStatus,
  resolutionReasoning: string | null,
): Promise<Insight> {
  return invoke<Insight>("orchd_set_insight_status", { id, status, resolutionReasoning });
}

export function orchdDeleteInsight(id: string): Promise<void> {
  return invoke<void>("orchd_delete_insight", { id });
}

export function orchdListInsights(projectId: string | null): Promise<Insight[]> {
  return invoke<Insight[]>("orchd_list_insights", { projectId });
}

// ── tasks ────────────────────────────────────────────────────────────────────────────────────

/** `priority: null` ⇒ the daemon defaults to `"normal"` (SCN-051 — mirrors `status: null` ⇒
 * `"backlog"`); callers that expose the priority control (`TasksList`'s create form) pass the
 * selected value explicitly. */
export function orchdCreateTask(
  projectId: string,
  parentId: string | null,
  title: string,
  body: string,
  status: TaskStatus | null,
  source: TaskSource,
  sourceId: string | null,
  tags: string[],
  priority: TaskPriority | null,
): Promise<DomainTask> {
  return invoke<DomainTask>("orchd_create_task", {
    projectId,
    parentId,
    title,
    body,
    status,
    source,
    sourceId,
    tags,
    priority,
  });
}

export function orchdUpdateTask(
  id: string,
  title: string | null,
  body: string | null,
  tags: string[] | null,
): Promise<DomainTask> {
  return invoke<DomainTask>("orchd_update_task", { id, title, body, tags });
}

export function orchdSetTaskStatus(id: string, status: TaskStatus): Promise<DomainTask> {
  return invoke<DomainTask>("orchd_set_task_status", { id, status });
}

export function orchdSetTaskRank(id: string, rank: number): Promise<DomainTask> {
  return invoke<DomainTask>("orchd_set_task_rank", { id, rank });
}

/** SCN-051 (ST-037): the urgent/normal flip on an existing task row — one focused command per
 * mutation, mirroring `orchdSetTaskStatus`/`orchdSetTaskRank` above. */
export function orchdSetTaskPriority(id: string, priority: TaskPriority): Promise<DomainTask> {
  return invoke<DomainTask>("orchd_set_task_priority", { id, priority });
}

export function orchdDeleteTask(id: string): Promise<void> {
  return invoke<void>("orchd_delete_task", { id });
}

export function orchdListTasks(projectId: string | null): Promise<DomainTask[]> {
  return invoke<DomainTask[]>("orchd_list_tasks", { projectId });
}

// ── research (S-IDEA §5/§6, task T6) — thin proxies over the 3 net-new research verbs ──────────
//
// Named WITHOUT the `orchd_` prefix, matching `commands.rs`'s own naming choice for this trio
// (spec §3 module-layout table: "research_start_run / research_list_runs / research_get_run
// (proxy)") — same argument-shaped-verbatim convention as every wrapper above.

/** Starts a research run (spec §6): inserts `research_run{pending}` and returns it immediately —
 * the run's terminal state (`done`/`failed`) arrives later via `orchd://research-runs-changed`
 * (`onOrchdResearchRunsChanged`, `./events.ts`), NOT this call's resolved value. */
export function researchStartRun(
  ideaId: string,
  serverId: string,
  toolName: string,
  argsJson: string,
): Promise<ResearchRun> {
  return invoke<ResearchRun>("research_start_run", { ideaId, serverId, toolName, argsJson });
}

/** Runs for one idea, newest first (spec §5). Plain read — never triggers a push. */
export function researchListRuns(ideaId: string): Promise<ResearchRun[]> {
  return invoke<ResearchRun[]>("research_list_runs", { ideaId });
}

/** One run by id (spec §5). Plain read — never triggers a push. */
export function researchGetRun(id: string): Promise<ResearchRun> {
  return invoke<ResearchRun>("research_get_run", { id });
}

// ── ruleset ──────────────────────────────────────────────────────────────────────────────────

export function orchdGetRuleset(scope: RuleScope, projectId: string | null): Promise<RuleSetView> {
  return invoke<RuleSetView>("orchd_get_ruleset", { scope, projectId });
}

export function orchdUpsertRuleset(
  scope: RuleScope,
  projectId: string | null,
  mdContent: string | null,
  mdPath: string | null,
  policy: PolicyRules | null,
): Promise<RuleSetView> {
  return invoke<RuleSetView>("orchd_upsert_ruleset", {
    scope,
    projectId,
    mdContent,
    mdPath,
    policy,
  });
}

export function orchdAcknowledgeRuleFile(id: string): Promise<RuleSetView> {
  return invoke<RuleSetView>("orchd_acknowledge_rule_file", { id });
}

// ── project docs (SCN-054, FLW-21, ST-041: the Docs tab) ─────────────────────────────────────
//
// One thin wrapper per `orchd_*_doc*` `#[tauri::command]` (`src-tauri/src/commands.rs`), same
// naming/arg-shape-verbatim convention as every wrapper above. Docs are "rules.md × N named
// files" — the wrapper set mirrors the ruleset block: `orchdUpsertDoc` is THE one write call
// (create "+ doc", Save, AND the "file lost" → Recreate path — exactly how `orchdUpsertRuleset`
// serves the rules editor); `orchdAcknowledgeDocFile` is the "file changed externally" banner's
// Accept; `orchdRevealDocFile` is CORE-ONLY path resolution (spec §9 "JS never passes a path" —
// the core re-derives `mdPath` from its own fresh `GetDoc`, see `reveal_doc_file_core`).

export function orchdListDocs(projectId: string): Promise<DocMeta[]> {
  return invoke<DocMeta[]>("orchd_list_docs", { projectId });
}

export function orchdGetDoc(projectId: string, name: string): Promise<DocView> {
  return invoke<DocView>("orchd_get_doc", { projectId, name });
}

export function orchdUpsertDoc(
  projectId: string,
  name: string,
  mdContent: string,
): Promise<DocView> {
  return invoke<DocView>("orchd_upsert_doc", { projectId, name, mdContent });
}

export function orchdDeleteDoc(id: string): Promise<void> {
  return invoke<void>("orchd_delete_doc", { id });
}

export function orchdAcknowledgeDocFile(id: string): Promise<DocView> {
  return invoke<DocView>("orchd_acknowledge_doc_file", { id });
}

/** Resolves once the OS file reveal (Finder) has been requested — a local action, deliberately
 * NOT gated on `orchdDown` in the UI beyond the read round-trip it needs (SCN-054: "reveal file"
 * stays live while orchd is up; the daemon read inside will reject honestly when it is not). */
export function orchdRevealDocFile(projectId: string, name: string): Promise<void> {
  return invoke<void>("orchd_reveal_doc_file", { projectId, name });
}

// ── graph (S4 §7, T5's orchd_graph_* commands) ──────────────────────────────────────────────
//
// One thin wrapper per `orchd_graph_*` `#[tauri::command]` (`src-tauri/src/commands.rs`, appended
// S4 T5), same naming/arg-shape-verbatim convention as every wrapper above. `orchdGraphDeleteNode`/
// `orchdGraphDeleteEdge` return `void` — they map to `OrchdRequest::GraphDeleteNode`/
// `GraphDeleteEdge` -> `OrchdResponse::Ack` on the Rust side (`expect_orchd_ack`), mirroring
// `orchdDeleteGoal`/`orchdDeleteTask` above.

export function orchdGraphAddNode(
  projectId: string,
  kind: GraphNodeKind,
  label: string,
  body: string,
  posX: number,
  posY: number,
): Promise<GraphNode> {
  return invoke<GraphNode>("orchd_graph_add_node", { projectId, kind, label, body, posX, posY });
}

export function orchdGraphUpdateNode(
  id: string,
  label: string | null,
  body: string | null,
): Promise<GraphNode> {
  return invoke<GraphNode>("orchd_graph_update_node", { id, label, body });
}

export function orchdGraphMoveNode(id: string, posX: number, posY: number): Promise<GraphNode> {
  return invoke<GraphNode>("orchd_graph_move_node", { id, posX, posY });
}

export function orchdGraphDeleteNode(id: string): Promise<void> {
  return invoke<void>("orchd_graph_delete_node", { id });
}

export function orchdGraphAddEdge(
  sourceNodeId: string,
  targetNodeId: string,
  kind: GraphEdgeKind,
  label: string,
): Promise<GraphEdge> {
  return invoke<GraphEdge>("orchd_graph_add_edge", { sourceNodeId, targetNodeId, kind, label });
}

export function orchdGraphUpdateEdge(id: string, kind: GraphEdgeKind): Promise<GraphEdge> {
  return invoke<GraphEdge>("orchd_graph_update_edge", { id, kind });
}

export function orchdGraphDeleteEdge(id: string): Promise<void> {
  return invoke<void>("orchd_graph_delete_edge", { id });
}

export function orchdGraphListProject(projectId: string): Promise<GraphView> {
  return invoke<GraphView>("orchd_graph_list_project", { projectId });
}

export function orchdGraphNeighborhood(
  nodeId: string,
  depth: number,
): Promise<GraphNeighborhood> {
  return invoke<GraphNeighborhood>("orchd_graph_neighborhood", { nodeId, depth });
}

export function orchdGraphSearch(query: string, projectId: string | null): Promise<GraphNode[]> {
  return invoke<GraphNode[]>("orchd_graph_search", { query, projectId });
}

// ── export / import ─────────────────────────────────────────────────────────────────────────

export function orchdExportProject(projectId: string): Promise<string> {
  return invoke<string>("orchd_export_project", { projectId });
}

export function orchdExportAll(): Promise<string> {
  return invoke<string>("orchd_export_all");
}

export function orchdImportBundle(json: string): Promise<ImportReport> {
  return invoke<ImportReport>("orchd_import_bundle", { json });
}

/**
 * CORE-ONLY (spec §9: "JS never passes a path" — the core re-derives the rules file's path from
 * its own fresh `GetRuleSet` reply, see `reveal_rules_file_core`). Resolves once the OS file
 * manager has been asked to reveal the file; never returns a path.
 */
export function orchdRevealRulesFile(scope: RuleScope, projectId: string | null): Promise<void> {
  return invoke<void>("orchd_reveal_rules_file", { scope, projectId });
}

/**
 * `destDir` is documented (spec §9) to be the exact `pickFolder()` result passed straight
 * through — never a freehand path. Resolves to the written file's absolute path.
 */
export function orchdExportToFile(projectId: string | null, destDir: string): Promise<string> {
  return invoke<string>("orchd_export_to_file", { projectId, destDir });
}

export function orchdImportFromFile(path: string): Promise<ImportReport> {
  return invoke<ImportReport>("orchd_import_from_file", { path });
}

// ── lifecycle ────────────────────────────────────────────────────────────────────────────────

/**
 * Drops and re-establishes the orchd connection (spec §9, the [Retry] retry action's
 * target). Fire-and-forget — the outcome is observed via `orchd://down|up|incompatible`
 * (`./events.ts`), not this command's resolved `Promise`.
 */
export function orchdReconnect(): Promise<void> {
  return invoke<void>("orchd_reconnect");
}

/**
 * Triggers the orchd upgrade flow (spec §9, mirrors `commands.ts`'s `upgradeDaemon` exactly for
 * the second daemon). The core kickstarts a new `bpa-orchd` and calls `app.restart()`, which
 * kills this webview process — this promise NEVER resolves on the happy path. Callers MUST treat
 * this as fire-and-forget but MUST attach a `.catch` (mirrors `upgradeDaemon`'s finding [13]
 * rationale): a REJECTED promise (`CommandError::UpgradeFailed`) is the one honest failure this
 * flow can surface.
 */
export function orchdUpgrade(): Promise<void> {
  return invoke<void>("orchd_upgrade");
}

// ── storage status (spec D3, BL-94) ────────────────────────────────────────────────────────────

/**
 * Reads the daemon's storage-degradation mode (spec D3, BL-94): whether it opened its on-disk DB
 * normally (`persistent`), fell back to a non-persistent in-memory DB (`in_memory_fallback`), or
 * recovered from a quarantined corrupt image (`recovered_from_corruption`, with `quarantinedPath`).
 * The mode is fixed at boot, so this is pulled once on connect and on every `orchd://up` reconnect
 * — there is no push. Proxies the `orchd_storage_status` `#[tauri::command]` (`commands.rs`),
 * `GetStorageStatus` → `StorageStatus` on the wire; a rejection is the caller's to map/gate (the
 * store's `refreshStorageStatus`), same as every wrapper above. */
export function orchdStorageStatus(): Promise<StorageStatus> {
  return invoke<StorageStatus>("orchd_storage_status");
}

// ── MCP (S-EXT §8, T7's mcp_*/trust_* commands) ─────────────────────────────────────────────
//
// One thin wrapper per `mcp_*`/`trust_*` `#[tauri::command]` (`src-tauri/src/commands.rs`, T7),
// same naming/arg-shape-verbatim convention as every wrapper above — argument order and names
// are copied straight from each Rust command's parameter list. `Option<T>` parameters are
// modeled as `T | null`, matching every wrapper above (never `undefined`).

export function mcpAddServer(
  name: string,
  transport: McpTransport,
  url: string | null,
  command: string | null,
  args: string[] | null,
  env: Record<string, string> | null,
  scope: McpScope,
  projectId: string | null,
  authKind: McpAuthKind,
  timeoutMs: number | null,
  maxRetries: number | null,
): Promise<McpServer> {
  return invoke<McpServer>("mcp_add_server", {
    name,
    transport,
    url,
    command,
    args,
    env,
    scope,
    projectId,
    authKind,
    timeoutMs,
    maxRetries,
  });
}

export function mcpListServers(projectId: string | null): Promise<McpServer[]> {
  return invoke<McpServer[]>("mcp_list_servers", { projectId });
}

export function mcpUpdateServer(
  id: string,
  name: string | null,
  url: string | null,
  command: string | null,
  args: string[] | null,
  env: Record<string, string> | null,
  authKind: McpAuthKind | null,
  timeoutMs: number | null,
  maxRetries: number | null,
): Promise<McpServer> {
  return invoke<McpServer>("mcp_update_server", {
    id,
    name,
    url,
    command,
    args,
    env,
    authKind,
    timeoutMs,
    maxRetries,
  });
}

export function mcpSetServerEnabled(id: string, enabled: boolean): Promise<McpServer> {
  return invoke<McpServer>("mcp_set_server_enabled", { id, enabled });
}

export function mcpDeleteServer(id: string): Promise<void> {
  return invoke<void>("mcp_delete_server", { id });
}

/**
 * `token` -> Keychain, ref -> DB, on the orchd side; this wrapper never logs or echoes it
 * (spec §5, mirrors `commands.rs::mcp_set_server_bearer`'s own doc comment verbatim).
 */
export function mcpSetServerBearer(id: string, token: string): Promise<void> {
  return invoke<void>("mcp_set_server_bearer", { id, token });
}

/** Trust-gated (spec D10): rejects with `CommandError{kind:"daemon",code:"Consent"}` when no
 * valid consent grant exists yet for this server's current URL — callers show `ConnectDialog`
 * on that rejection (`ServersTab.tsx`). */
export function mcpConnect(id: string): Promise<McpConnectReport> {
  return invoke<McpConnectReport>("mcp_connect", { id });
}

export function mcpDisconnect(id: string): Promise<void> {
  return invoke<void>("mcp_disconnect", { id });
}

export function mcpListTools(serverId: string): Promise<McpTool[]> {
  return invoke<McpTool[]>("mcp_list_tools", { serverId });
}

/** Per-tool allowlist toggle (S0/S1 §16) — note the Rust param is `tool_id`, not `id`. */
export function mcpSetToolEnabled(toolId: string, enabled: boolean): Promise<McpTool> {
  return invoke<McpTool>("mcp_set_tool_enabled", { toolId, enabled });
}

/** Rejects with `CommandError{kind:"daemon",code:"Policy"}` BEFORE dispatch when the named tool
 * is disabled (spec §6 per-tool allowlist) — never a silent no-op. */
export function mcpCallTool(
  serverId: string,
  toolName: string,
  argsJson: string,
  projectId: string | null,
): Promise<McpCallResult> {
  return invoke<McpCallResult>("mcp_call_tool", { serverId, toolName, argsJson, projectId });
}

export function mcpListInvocations(
  serverId: string | null,
  projectId: string | null,
  limit: number | null,
): Promise<McpInvocation[]> {
  return invoke<McpInvocation[]>("mcp_list_invocations", { serverId, projectId, limit });
}

export function mcpListArtifacts(
  projectId: string | null,
  serverId: string | null,
  limit: number | null,
): Promise<McpArtifact[]> {
  return invoke<McpArtifact[]>("mcp_list_artifacts", { projectId, serverId, limit });
}

export function mcpGetArtifact(id: string): Promise<McpArtifact> {
  return invoke<McpArtifact>("mcp_get_artifact", { id });
}

/** Grants an owner consent (spec D10, `consent_grant` table): `kind` is `"connect"` for a
 * server's first `mcpConnect` (`"stdio_exec"` is Phase 3, not surfaced yet). Idempotent
 * (`Db::grant_consent` upserts on `(server_id, kind)` — a re-grant just refreshes the
 * fingerprint), so callers may call this unconditionally before every connect attempt. */
export function trustGrantConsent(serverId: string, kind: string): Promise<void> {
  return invoke<void>("trust_grant_consent", { serverId, kind });
}

// ── Connectors (S-EXT §5/§7/§8, T13a's connector_* commands) ───────────────────────────────────
//
// One thin wrapper per `connector_*` `#[tauri::command]` (`src-tauri/src/commands.rs`, T13a).
// Unlike every wrapper above, each of these seven takes ONE destructured options object rather
// than positional params — deliberate for this family (T13b brief): several verbs share
// argument NAMES with different verbs' UNRELATED positions (`id` on `connectorDeleteAccount` vs.
// `accountId` everywhere else), so a single object per call reads unambiguously at every call
// site. The object's OWN keys are still copied straight into `invoke()`'s payload object
// (Tauri maps each camelCase JS key to its Rust snake_case parameter, same convention as every
// wrapper above) — `provider`/`label`/`apiKey`/`accountId`/`op`/`argsJson`/`id` all match T13a's
// Rust parameter names verbatim. Optional fields (`scopes`, `serverId`, `projectId`) are `T | null`
// on the wire even though the object's own TS type marks them optional (`?`) for a nicer call
// site — an omitted key becomes `null`, never `undefined`, matching every `Option<T>` wrapper
// above.
//
// `apiKey` (`connectorAddApiKey`) and `code` (`connectorCompleteOAuth`) are passed straight
// through to `invoke()` and never logged/echoed by this module (spec §5/§6, mirrors
// `mcpSetServerBearer`'s doc above verbatim).

/** Begins an OAuth 2.1 + PKCE flow for a new connector account (spec §5/§7). Returns the
 * `authorizeUrl` to open in the browser plus the CSRF `state` `connectorCompleteOAuth` must echo
 * back. `serverId` links this account to an existing OAuth-authenticated MCP server (spec D5) —
 * omit it (or pass `undefined`) for a standalone/direct-API connector account. */
export function connectorBeginOAuth(args: {
  provider: string;
  label: string;
  scopes?: string[];
  serverId?: string;
}): Promise<OAuthChallenge> {
  return invoke<OAuthChallenge>("connector_begin_oauth", {
    provider: args.provider,
    label: args.label,
    scopes: args.scopes ?? null,
    serverId: args.serverId ?? null,
  });
}

/**
 * Completes the PKCE round-trip: `code` -> exchanged for tokens on the orchd side (Keychain);
 * this wrapper never logs or echoes it (spec §5/§6). The wire arg key is `oauthState` — T13a's
 * Rust parameter is named `oauth_state`, NOT `state` (that name is reserved on the Rust side for
 * Tauri's own injected `State<'_, AppState>`) — but this wrapper's OWN parameter stays named
 * `state` so callers can pass `OAuthChallenge.state` straight through without renaming it.
 */
export function connectorCompleteOAuth(args: { state: string; code: string }): Promise<Account> {
  return invoke<Account>("connector_complete_oauth", { oauthState: args.state, code: args.code });
}

/** `apiKey` -> Keychain, ref -> DB, on the orchd side; never logged or echoed by this module
 * (spec §5/§6, mirrors `mcpSetServerBearer`'s doc above). */
export function connectorAddApiKey(args: {
  provider: string;
  label: string;
  apiKey: string;
}): Promise<Account> {
  return invoke<Account>("connector_add_api_key", {
    provider: args.provider,
    label: args.label,
    apiKey: args.apiKey,
  });
}

export function connectorListAccounts(): Promise<Account[]> {
  return invoke<Account[]>("connector_list_accounts");
}

export function connectorDeleteAccount(args: { id: string }): Promise<void> {
  return invoke<void>("connector_delete_account", { id: args.id });
}

export function connectorListOps(args: { accountId: string }): Promise<ConnectorOp[]> {
  return invoke<ConnectorOp[]>("connector_list_ops", { accountId: args.accountId });
}

/** Lists the NAMES of the OAuth providers configured in `<app-support>/oauth_providers.json`
 * (spec D7, O-5) — feeds the OAuth-provider dropdown in `ConnectorsTab`. Names only; no client
 * id/secret/URLs cross the wire. An empty array is the honest "no providers configured" state. */
export function connectorListProviders(): Promise<string[]> {
  return invoke<string[]>("connector_list_providers");
}

/** Trust-gated IDENTICALLY to `mcpCallTool` (spec §6/§7): a spend/rate-cap denial rejects with
 * `CommandError{kind:"daemon",code:"Policy"}` BEFORE dispatch, and the result persists as a
 * durable `is_untrusted:true` artifact on success (spec D9) — same "never a silent no-op"
 * contract as `mcpCallTool` above. */
export function connectorInvoke(args: {
  accountId: string;
  op: string;
  argsJson: string;
  projectId?: string;
}): Promise<McpCallResult> {
  return invoke<McpCallResult>("connector_invoke", {
    accountId: args.accountId,
    op: args.op,
    argsJson: args.argsJson,
    projectId: args.projectId ?? null,
  });
}

// ── Skills (S-EXT §5/§8, D11, Q14, T17's skill_* commands) ──────────────────────────────────
//
// One thin wrapper per `skill_*` `#[tauri::command]` (`src-tauri/src/commands.rs`, T17), same
// naming/arg-shape-verbatim convention as every wrapper above. PLUMBING ONLY (D11): this registry
// has no runtime consumer until the S6b agent org — see `SkillsTab.tsx`'s banner.

/** `name`/`description: null` ⇒ parsed from the SKILL.md frontmatter at `mdPath` (Q14) on the
 * orchd side; rejects with `CommandError{kind:"daemon",code:"Validation"}` when NEITHER an
 * explicit override NOR a parseable frontmatter name is available. */
export function skillAdd(
  name: string | null,
  description: string | null,
  mdPath: string,
  scope: SkillScope,
  projectId: string | null,
): Promise<Skill> {
  return invoke<Skill>("skill_add", { name, description, mdPath, scope, projectId });
}

export function skillList(projectId: string | null): Promise<Skill[]> {
  return invoke<Skill[]>("skill_list", { projectId });
}

export function skillDelete(id: string): Promise<void> {
  return invoke<void>("skill_delete", { id });
}

// ── Trust: policy caps + audit log (S-EXT §4/§5/§6, BL-22, T18's trust_* commands) ─────────────
//
// One thin wrapper per `trust_*` `#[tauri::command]` (`src-tauri/src/commands.rs`, T18), same
// naming/arg-shape-verbatim convention as every wrapper above.

/** UPSERT keyed by `(scope, refId)` (spec §4) — `scope:"global"` requires `refId: null`,
 * `scope:"project"|"server"` requires `refId` set; rejects with
 * `CommandError{kind:"daemon",code:"Validation"}` on a mismatch, BEFORE any row is written.
 * `null` cap fields mean "unlimited" for that dimension. */
export function trustSetPolicy(
  scope: PolicyScope,
  refId: string | null,
  spendCapUsd: number | null,
  ratePerMin: number | null,
): Promise<Policy> {
  return invoke<Policy>("trust_set_policy", { scope, refId, spendCapUsd, ratePerMin });
}

export function trustListPolicies(): Promise<Policy[]> {
  return invoke<Policy[]>("trust_list_policies");
}

/** Newest-first, optionally capped at `limit` — every trust-choke-point decision, allow or
 * deny, for the Log/audit UI. */
export function trustListAudit(limit: number | null): Promise<AuditRow[]> {
  return invoke<AuditRow[]>("trust_list_audit", { limit });
}

// ── error mapping ────────────────────────────────────────────────────────────────────────────

/**
 * Human-readable English message for a rejected `orchd_*` call (spec §9/§10 "honest error
 * surface"). Mirrors the `CommandError` shape `src-tauri/src/commands.rs` serializes
 * (`#[serde(tag = "kind", rename_all = "camelCase")]`; there is no hand-written `CommandError` TS
 * type yet — same situation `commands.ts`'s `DaemonStatus` doc notes — so this reads the shape
 * defensively off `unknown`).
 *
 * `kind: "daemon"`'s `code` is `format!("{code:?}")` of `bpa_orchd_proto::OrchdErrorCode` (Rust
 * `Debug`, e.g. `"Invariant"` — see `err_from_orchd_response` in `commands.rs`), NOT the
 * lower-camelCase `OrchdErrorCode` ts-rs union in `./orchd-types.ts` (that union is `serde`'s
 * `Serialize` casing, used only when an `OrchdErrorCode` itself is a struct field on the wire —
 * unrelated to this Debug-formatted string). `Consent`/`Policy` are the two S-EXT-only codes
 * (spec §5/§6, T7): `Consent` is `McpConnect`'s trust-choke-point denial (no valid consent grant
 * yet for the server's current URL — `ConnectDialog.tsx` is the intended recovery, not merely a
 * toast), `Policy` is `McpCallTool`'s denial (a disabled tool, the per-tool allowlist). Every
 * other `kind` (`disconnected`, `incompatibleOrchd`, and anything unrecognized) falls back to an
 * honest generic message rather than guessing.
 */
export function describeOrchdError(e: unknown): string {
  if (e !== null && typeof e === "object" && "kind" in e) {
    const err = e as { kind: unknown; code?: unknown; message?: unknown };
    if (err.kind === "daemon") {
      const code = typeof err.code === "string" ? err.code : "";
      const message = typeof err.message === "string" ? err.message : "";
      switch (code) {
        case "Invariant":
          return strings.errors.invariant(message);
        case "Conflict":
          return strings.errors.conflict(message);
        case "NotFound":
          return strings.errors.notFound;
        case "Validation":
          return strings.errors.validation(message);
        case "Io":
          return strings.errors.io(message);
        case "Consent":
          return strings.errors.consent(message);
        case "Policy":
          return strings.errors.policy(message);
        default:
          return message || strings.errors.orchdError;
      }
    }
    if (err.kind === "disconnected" || err.kind === "incompatibleOrchd") {
      return strings.errors.unavailable;
    }
  }
  return strings.errors.unknown;
}

/**
 * `true` when `e` is a trust-choke-point `Consent` denial — i.e. a rejected `CommandError` whose
 * `kind === "daemon"` and Debug-formatted `code === "Consent"` (the exact shape
 * `describeOrchdError` reads above; `McpConnect`/`McpCallTool`/`ConnectorInvoke` reject with it when
 * no valid consent grant exists yet for the server's current URL). The only recovery is
 * `ConnectDialog`, which is reachable ONLY from the Servers tab — so callers append
 * `strings.errors.consentRecovery` to the surfaced message pointing there (P-20), instead of
 * dead-ending on a toast with no path forward.
 */
export function isConsentError(e: unknown): boolean {
  return (
    e !== null &&
    typeof e === "object" &&
    "kind" in e &&
    (e as { kind: unknown }).kind === "daemon" &&
    "code" in e &&
    (e as { code: unknown }).code === "Consent"
  );
}

/**
 * `true` when `e` is a daemon `NotFound` rejection — same `CommandError` shape reading as
 * `isConsentError` above. Consumed by the store's `refreshDoc` (SCN-054): when a doc-changed
 * push races a concurrent delete from another client, the stale view's re-fetch rejects
 * `NotFound` — that is the honest "this doc is gone" signal (the view entry is dropped), not an
 * error worth a toast.
 */
export function isNotFoundError(e: unknown): boolean {
  return (
    e !== null &&
    typeof e === "object" &&
    "kind" in e &&
    (e as { kind: unknown }).kind === "daemon" &&
    "code" in e &&
    (e as { code: unknown }).code === "NotFound"
  );
}
