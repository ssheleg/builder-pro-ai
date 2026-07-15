import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceId } from "./commands";
import type {
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
  PolicyRules,
  Project,
  RuleScope,
  RuleSetView,
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

export function orchdCreateTask(
  projectId: string,
  parentId: string | null,
  title: string,
  body: string,
  status: TaskStatus | null,
  source: TaskSource,
  sourceId: string | null,
  tags: string[],
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

export function orchdDeleteTask(id: string): Promise<void> {
  return invoke<void>("orchd_delete_task", { id });
}

export function orchdListTasks(projectId: string | null): Promise<DomainTask[]> {
  return invoke<DomainTask[]>("orchd_list_tasks", { projectId });
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
 * Drops and re-establishes the orchd connection (spec §9, the [Повторить] retry action's
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

// ── error mapping ────────────────────────────────────────────────────────────────────────────

/**
 * Human-readable Russian message for a rejected `orchd_*` call (spec §9/§10 "honest error
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
          return `недопустимая операция: ${message}`;
        case "Conflict":
          return `конфликт: ${message}`;
        case "NotFound":
          return "не найдено";
        case "Validation":
          return `неверные данные: ${message}`;
        case "Io":
          return `ошибка сервиса: ${message}`;
        case "Consent":
          return `требуется согласие на подключение: ${message}`;
        case "Policy":
          return `запрещено политикой: ${message}`;
        default:
          return message || "ошибка оркестратора";
      }
    }
    if (err.kind === "disconnected" || err.kind === "incompatibleOrchd") {
      return "оркестратор недоступен";
    }
  }
  return "неизвестная ошибка оркестратора";
}
