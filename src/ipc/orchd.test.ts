import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
}));

import {
  orchdPing,
  orchdCreateProject,
  orchdUpdateProject,
  orchdArchiveProject,
  orchdListProjects,
  orchdAddProjectWorkspace,
  orchdRemoveProjectWorkspace,
  orchdCreateGoal,
  orchdUpdateGoal,
  orchdMoveGoal,
  orchdDeleteGoal,
  orchdListGoals,
  orchdCreateIdea,
  orchdUpdateIdea,
  orchdSetIdeaProject,
  orchdSetIdeaLifecycle,
  orchdDeleteIdea,
  orchdListIdeas,
  orchdCreateInsight,
  orchdUpdateInsight,
  orchdSetInsightFitVerdict,
  orchdSetInsightStatus,
  orchdDeleteInsight,
  orchdListInsights,
  orchdCreateTask,
  orchdUpdateTask,
  orchdSetTaskStatus,
  orchdSetTaskRank,
  orchdDeleteTask,
  orchdListTasks,
  orchdGetRuleset,
  orchdUpsertRuleset,
  orchdAcknowledgeRuleFile,
  orchdGraphAddNode,
  orchdGraphUpdateNode,
  orchdGraphMoveNode,
  orchdGraphDeleteNode,
  orchdGraphAddEdge,
  orchdGraphDeleteEdge,
  orchdGraphListProject,
  orchdGraphNeighborhood,
  orchdGraphSearch,
  orchdExportProject,
  orchdExportAll,
  orchdImportBundle,
  orchdRevealRulesFile,
  orchdExportToFile,
  orchdImportFromFile,
  orchdReconnect,
  orchdUpgrade,
  mcpAddServer,
  mcpListServers,
  mcpUpdateServer,
  mcpSetServerEnabled,
  mcpDeleteServer,
  mcpSetServerBearer,
  mcpConnect,
  mcpDisconnect,
  mcpListTools,
  mcpSetToolEnabled,
  mcpCallTool,
  mcpListInvocations,
  mcpListArtifacts,
  mcpGetArtifact,
  trustGrantConsent,
  connectorBeginOAuth,
  connectorCompleteOAuth,
  connectorAddApiKey,
  connectorListAccounts,
  connectorDeleteAccount,
  connectorListOps,
  connectorInvoke,
  skillAdd,
  skillList,
  skillDelete,
  trustSetPolicy,
  trustListPolicies,
  trustListAudit,
  describeOrchdError,
} from "./orchd";

describe("ipc/orchd", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("orchdPing calls orchd_ping with no args", async () => {
    await orchdPing();
    expect(invokeMock).toHaveBeenCalledWith("orchd_ping");
  });

  // ── projects ───────────────────────────────────────────────────────────────────────────────

  it("orchdCreateProject sends name/description/workspaceIds", async () => {
    await orchdCreateProject("Acme", "desc", ["w1", "w2"]);
    expect(invokeMock).toHaveBeenCalledWith("orchd_create_project", {
      name: "Acme",
      description: "desc",
      workspaceIds: ["w1", "w2"],
    });
  });

  it("orchdUpdateProject sends id/name/description (null = unchanged)", async () => {
    await orchdUpdateProject("p1", "New name", null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_update_project", {
      id: "p1",
      name: "New name",
      description: null,
    });
  });

  it("orchdArchiveProject sends id", async () => {
    await orchdArchiveProject("p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_archive_project", { id: "p1" });
  });

  it("orchdListProjects calls orchd_list_projects with no args", async () => {
    await orchdListProjects();
    expect(invokeMock).toHaveBeenCalledWith("orchd_list_projects");
  });

  it("orchdAddProjectWorkspace sends projectId/workspaceId", async () => {
    await orchdAddProjectWorkspace("p1", "w1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_add_project_workspace", {
      projectId: "p1",
      workspaceId: "w1",
    });
  });

  it("orchdRemoveProjectWorkspace sends projectId/workspaceId", async () => {
    await orchdRemoveProjectWorkspace("p1", "w1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_remove_project_workspace", {
      projectId: "p1",
      workspaceId: "w1",
    });
  });

  // ── goals ──────────────────────────────────────────────────────────────────────────────────

  it("orchdCreateGoal sends projectId/parentId/kind/title/body", async () => {
    await orchdCreateGoal("p1", null, "strategic", "Ship v1", "body");
    expect(invokeMock).toHaveBeenCalledWith("orchd_create_goal", {
      projectId: "p1",
      parentId: null,
      kind: "strategic",
      title: "Ship v1",
      body: "body",
    });
  });

  it("orchdUpdateGoal sends id/title/body/status/metricRefs", async () => {
    await orchdUpdateGoal("g1", "New title", null, "achieved", ["m1"]);
    expect(invokeMock).toHaveBeenCalledWith("orchd_update_goal", {
      id: "g1",
      title: "New title",
      body: null,
      status: "achieved",
      metricRefs: ["m1"],
    });
  });

  it("orchdMoveGoal sends id/newParentId/newOrd", async () => {
    await orchdMoveGoal("g1", "g0", 2);
    expect(invokeMock).toHaveBeenCalledWith("orchd_move_goal", {
      id: "g1",
      newParentId: "g0",
      newOrd: 2,
    });
  });

  it("orchdDeleteGoal sends id", async () => {
    await orchdDeleteGoal("g1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_delete_goal", { id: "g1" });
  });

  it("orchdListGoals sends projectId", async () => {
    await orchdListGoals("p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_list_goals", { projectId: "p1" });
  });

  // ── ideas ──────────────────────────────────────────────────────────────────────────────────

  it("orchdCreateIdea sends projectId/title/body (projectId may be null — unassigned)", async () => {
    await orchdCreateIdea(null, "Idea title", "body");
    expect(invokeMock).toHaveBeenCalledWith("orchd_create_idea", {
      projectId: null,
      title: "Idea title",
      body: "body",
    });
  });

  it("orchdUpdateIdea sends id/title/body", async () => {
    await orchdUpdateIdea("i1", "New title", null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_update_idea", {
      id: "i1",
      title: "New title",
      body: null,
    });
  });

  it("orchdSetIdeaProject sends id/projectId", async () => {
    await orchdSetIdeaProject("i1", "p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_idea_project", {
      id: "i1",
      projectId: "p1",
    });
  });

  it("orchdSetIdeaLifecycle sends id/lifecycle", async () => {
    await orchdSetIdeaLifecycle("i1", "specced");
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_idea_lifecycle", {
      id: "i1",
      lifecycle: "specced",
    });
  });

  it("orchdDeleteIdea sends id", async () => {
    await orchdDeleteIdea("i1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_delete_idea", { id: "i1" });
  });

  it("orchdListIdeas sends projectId (null = every project)", async () => {
    await orchdListIdeas(null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_list_ideas", { projectId: null });
  });

  // ── insights ───────────────────────────────────────────────────────────────────────────────

  it("orchdCreateInsight sends projectId/source/title/body", async () => {
    await orchdCreateInsight("p1", "user-interview", "Insight title", "body");
    expect(invokeMock).toHaveBeenCalledWith("orchd_create_insight", {
      projectId: "p1",
      source: "user-interview",
      title: "Insight title",
      body: "body",
    });
  });

  it("orchdUpdateInsight sends id/title/body", async () => {
    await orchdUpdateInsight("in1", null, "New body");
    expect(invokeMock).toHaveBeenCalledWith("orchd_update_insight", {
      id: "in1",
      title: null,
      body: "New body",
    });
  });

  it("orchdSetInsightFitVerdict sends id/fitVerdict/fitReasoning", async () => {
    await orchdSetInsightFitVerdict("in1", "fit", "because reasons");
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_insight_fit_verdict", {
      id: "in1",
      fitVerdict: "fit",
      fitReasoning: "because reasons",
    });
  });

  it("orchdSetInsightStatus sends id/status/resolutionReasoning", async () => {
    await orchdSetInsightStatus("in1", "archived", "no longer relevant");
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_insight_status", {
      id: "in1",
      status: "archived",
      resolutionReasoning: "no longer relevant",
    });
  });

  it("orchdDeleteInsight sends id", async () => {
    await orchdDeleteInsight("in1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_delete_insight", { id: "in1" });
  });

  it("orchdListInsights sends projectId", async () => {
    await orchdListInsights("p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_list_insights", { projectId: "p1" });
  });

  // ── tasks ──────────────────────────────────────────────────────────────────────────────────

  it("orchdCreateTask sends every field in Rust param order", async () => {
    await orchdCreateTask("p1", null, "Task title", "body", "todo", "idea", "i1", ["tag1"]);
    expect(invokeMock).toHaveBeenCalledWith("orchd_create_task", {
      projectId: "p1",
      parentId: null,
      title: "Task title",
      body: "body",
      status: "todo",
      source: "idea",
      sourceId: "i1",
      tags: ["tag1"],
    });
  });

  it("orchdUpdateTask sends id/title/body/tags", async () => {
    await orchdUpdateTask("t1", "New title", null, null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_update_task", {
      id: "t1",
      title: "New title",
      body: null,
      tags: null,
    });
  });

  it("orchdSetTaskStatus sends id/status", async () => {
    await orchdSetTaskStatus("t1", "done");
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_task_status", {
      id: "t1",
      status: "done",
    });
  });

  it("orchdSetTaskRank sends id/rank", async () => {
    await orchdSetTaskRank("t1", 1.5);
    expect(invokeMock).toHaveBeenCalledWith("orchd_set_task_rank", { id: "t1", rank: 1.5 });
  });

  it("orchdDeleteTask sends id", async () => {
    await orchdDeleteTask("t1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_delete_task", { id: "t1" });
  });

  it("orchdListTasks sends projectId", async () => {
    await orchdListTasks(null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_list_tasks", { projectId: null });
  });

  // ── ruleset ────────────────────────────────────────────────────────────────────────────────

  it("orchdGetRuleset sends scope/projectId", async () => {
    await orchdGetRuleset("global", null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_get_ruleset", {
      scope: "global",
      projectId: null,
    });
  });

  it("orchdUpsertRuleset sends scope/projectId/mdContent/mdPath/policy", async () => {
    const policy = { spendCapUsd: 10, approvalClasses: ["deploy"], pathAllowlist: ["/x"] };
    await orchdUpsertRuleset("project", "p1", "# rules", "/path/rules.md", policy);
    expect(invokeMock).toHaveBeenCalledWith("orchd_upsert_ruleset", {
      scope: "project",
      projectId: "p1",
      mdContent: "# rules",
      mdPath: "/path/rules.md",
      policy,
    });
  });

  it("orchdAcknowledgeRuleFile sends id", async () => {
    await orchdAcknowledgeRuleFile("r1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_acknowledge_rule_file", { id: "r1" });
  });

  // ── graph ──────────────────────────────────────────────────────────────────────────────────

  it("orchdGraphAddNode sends projectId/kind/label/body/posX/posY", async () => {
    await orchdGraphAddNode("p1", "concept", "Node label", "body", 10, 20);
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_add_node", {
      projectId: "p1",
      kind: "concept",
      label: "Node label",
      body: "body",
      posX: 10,
      posY: 20,
    });
  });

  it("orchdGraphUpdateNode sends id/label/body", async () => {
    await orchdGraphUpdateNode("n1", "New label", null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_update_node", {
      id: "n1",
      label: "New label",
      body: null,
    });
  });

  it("orchdGraphMoveNode sends id/posX/posY", async () => {
    await orchdGraphMoveNode("n1", 5, 9);
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_move_node", {
      id: "n1",
      posX: 5,
      posY: 9,
    });
  });

  it("orchdGraphDeleteNode sends id", async () => {
    await orchdGraphDeleteNode("n1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_delete_node", { id: "n1" });
  });

  it("orchdGraphAddEdge sends sourceNodeId/targetNodeId/kind/label", async () => {
    await orchdGraphAddEdge("n1", "n2", "depends", "blocks");
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_add_edge", {
      sourceNodeId: "n1",
      targetNodeId: "n2",
      kind: "depends",
      label: "blocks",
    });
  });

  it("orchdGraphDeleteEdge sends id", async () => {
    await orchdGraphDeleteEdge("e1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_delete_edge", { id: "e1" });
  });

  it("orchdGraphListProject sends projectId", async () => {
    await orchdGraphListProject("p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_list_project", { projectId: "p1" });
  });

  it("orchdGraphNeighborhood sends nodeId/depth", async () => {
    await orchdGraphNeighborhood("n1", 2);
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_neighborhood", {
      nodeId: "n1",
      depth: 2,
    });
  });

  it("orchdGraphSearch sends query/projectId", async () => {
    await orchdGraphSearch("hello", "p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_search", {
      query: "hello",
      projectId: "p1",
    });
  });

  it("orchdGraphSearch sends projectId: null for a workspace-wide search", async () => {
    await orchdGraphSearch("hello", null);
    expect(invokeMock).toHaveBeenCalledWith("orchd_graph_search", {
      query: "hello",
      projectId: null,
    });
  });

  // ── export / import ───────────────────────────────────────────────────────────────────────

  it("orchdExportProject sends projectId, resolves the export JSON string", async () => {
    invokeMock.mockResolvedValueOnce("{}");
    const res = await orchdExportProject("p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_export_project", { projectId: "p1" });
    expect(res).toBe("{}");
  });

  it("orchdExportAll calls orchd_export_all with no args", async () => {
    await orchdExportAll();
    expect(invokeMock).toHaveBeenCalledWith("orchd_export_all");
  });

  it("orchdImportBundle sends json, resolves an ImportReport", async () => {
    const report = { projects: 1, goals: 2, ideas: 0, insights: 0, tasks: 3, rulesets: 1 };
    invokeMock.mockResolvedValueOnce(report);
    const res = await orchdImportBundle("{\"bundleFormat\":1}");
    expect(invokeMock).toHaveBeenCalledWith("orchd_import_bundle", {
      json: "{\"bundleFormat\":1}",
    });
    expect(res).toEqual(report);
  });

  it("orchdRevealRulesFile sends scope/projectId", async () => {
    await orchdRevealRulesFile("project", "p1");
    expect(invokeMock).toHaveBeenCalledWith("orchd_reveal_rules_file", {
      scope: "project",
      projectId: "p1",
    });
  });

  it("orchdExportToFile sends projectId/destDir, resolves the written path", async () => {
    invokeMock.mockResolvedValueOnce("/dest/store-export.json");
    const res = await orchdExportToFile(null, "/dest");
    expect(invokeMock).toHaveBeenCalledWith("orchd_export_to_file", {
      projectId: null,
      destDir: "/dest",
    });
    expect(res).toBe("/dest/store-export.json");
  });

  it("orchdImportFromFile sends path, resolves an ImportReport", async () => {
    const report = { projects: 1, goals: 0, ideas: 0, insights: 0, tasks: 0, rulesets: 0 };
    invokeMock.mockResolvedValueOnce(report);
    const res = await orchdImportFromFile("/dest/store-export.json");
    expect(invokeMock).toHaveBeenCalledWith("orchd_import_from_file", {
      path: "/dest/store-export.json",
    });
    expect(res).toEqual(report);
  });

  // ── lifecycle ──────────────────────────────────────────────────────────────────────────────

  it("orchdReconnect calls orchd_reconnect with no args", async () => {
    await orchdReconnect();
    expect(invokeMock).toHaveBeenCalledWith("orchd_reconnect");
  });

  it("orchdUpgrade calls orchd_upgrade with no args", async () => {
    await orchdUpgrade();
    expect(invokeMock).toHaveBeenCalledWith("orchd_upgrade");
  });

  it("orchdUpgrade propagates a rejected UpgradeFailed CommandError as-is (finding [13] parity)", async () => {
    const err = { kind: "upgradeFailed", reason: "Operation not permitted" };
    invokeMock.mockRejectedValueOnce(err);
    await expect(orchdUpgrade()).rejects.toEqual(err);
  });

  // ── MCP (S-EXT §8, T8) ─────────────────────────────────────────────────────────────────────

  it("mcpAddServer sends every field camelCased, in T7's mcp_add_server param order", async () => {
    await mcpAddServer(
      "Prowl",
      "http",
      "https://prowl.chat/mcp",
      null,
      null,
      null,
      "global",
      null,
      "bearer",
      30000,
      2,
    );
    expect(invokeMock).toHaveBeenCalledWith("mcp_add_server", {
      name: "Prowl",
      transport: "http",
      url: "https://prowl.chat/mcp",
      command: null,
      args: null,
      env: null,
      scope: "global",
      projectId: null,
      authKind: "bearer",
      timeoutMs: 30000,
      maxRetries: 2,
    });
  });

  it("mcpListServers sends projectId", async () => {
    await mcpListServers(null);
    expect(invokeMock).toHaveBeenCalledWith("mcp_list_servers", { projectId: null });
  });

  it("mcpUpdateServer sends id + editable fields (null = unchanged)", async () => {
    await mcpUpdateServer("s1", "New name", null, null, null, null, null, null, null);
    expect(invokeMock).toHaveBeenCalledWith("mcp_update_server", {
      id: "s1",
      name: "New name",
      url: null,
      command: null,
      args: null,
      env: null,
      authKind: null,
      timeoutMs: null,
      maxRetries: null,
    });
  });

  it("mcpSetServerEnabled sends id/enabled", async () => {
    await mcpSetServerEnabled("s1", false);
    expect(invokeMock).toHaveBeenCalledWith("mcp_set_server_enabled", { id: "s1", enabled: false });
  });

  it("mcpDeleteServer sends id", async () => {
    await mcpDeleteServer("s1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_delete_server", { id: "s1" });
  });

  it("mcpSetServerBearer sends id/token", async () => {
    await mcpSetServerBearer("s1", "sekret-token");
    expect(invokeMock).toHaveBeenCalledWith("mcp_set_server_bearer", {
      id: "s1",
      token: "sekret-token",
    });
  });

  it("mcpConnect sends id", async () => {
    await mcpConnect("s1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_connect", { id: "s1" });
  });

  it("mcpDisconnect sends id", async () => {
    await mcpDisconnect("s1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_disconnect", { id: "s1" });
  });

  it("mcpListTools sends serverId", async () => {
    await mcpListTools("s1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_list_tools", { serverId: "s1" });
  });

  it("mcpSetToolEnabled sends toolId (not id)/enabled", async () => {
    await mcpSetToolEnabled("t1", true);
    expect(invokeMock).toHaveBeenCalledWith("mcp_set_tool_enabled", { toolId: "t1", enabled: true });
  });

  it("mcpCallTool sends serverId/toolName/argsJson/projectId", async () => {
    await mcpCallTool("s1", "search", '{"q":"x"}', "p1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_call_tool", {
      serverId: "s1",
      toolName: "search",
      argsJson: '{"q":"x"}',
      projectId: "p1",
    });
  });

  it("mcpListInvocations sends serverId/projectId/limit", async () => {
    await mcpListInvocations(null, null, 50);
    expect(invokeMock).toHaveBeenCalledWith("mcp_list_invocations", {
      serverId: null,
      projectId: null,
      limit: 50,
    });
  });

  it("mcpListArtifacts sends projectId/serverId/limit", async () => {
    await mcpListArtifacts(null, null, null);
    expect(invokeMock).toHaveBeenCalledWith("mcp_list_artifacts", {
      projectId: null,
      serverId: null,
      limit: null,
    });
  });

  it("mcpGetArtifact sends id", async () => {
    await mcpGetArtifact("a1");
    expect(invokeMock).toHaveBeenCalledWith("mcp_get_artifact", { id: "a1" });
  });

  it("trustGrantConsent sends serverId/kind", async () => {
    await trustGrantConsent("s1", "connect");
    expect(invokeMock).toHaveBeenCalledWith("trust_grant_consent", { serverId: "s1", kind: "connect" });
  });

  // ── Connectors (S-EXT §5/§7/§8, T13a's connector_* commands, T13b) ────────────────────────────

  it("connectorBeginOAuth sends provider/label/scopes/serverId, defaulting the optionals to null", async () => {
    await connectorBeginOAuth({ provider: "github", label: "My GitHub" });
    expect(invokeMock).toHaveBeenCalledWith("connector_begin_oauth", {
      provider: "github",
      label: "My GitHub",
      scopes: null,
      serverId: null,
    });
  });

  it("connectorBeginOAuth passes scopes/serverId through when given", async () => {
    await connectorBeginOAuth({
      provider: "github",
      label: "My GitHub",
      scopes: ["repo", "user"],
      serverId: "srv-1",
    });
    expect(invokeMock).toHaveBeenCalledWith("connector_begin_oauth", {
      provider: "github",
      label: "My GitHub",
      scopes: ["repo", "user"],
      serverId: "srv-1",
    });
  });

  it("connectorCompleteOAuth sends oauthState (not state) + code — T13a's Rust param is oauth_state", async () => {
    await connectorCompleteOAuth({ state: "st-1", code: "code-xyz" });
    expect(invokeMock).toHaveBeenCalledWith("connector_complete_oauth", {
      oauthState: "st-1",
      code: "code-xyz",
    });
  });

  it("connectorAddApiKey sends provider/label/apiKey", async () => {
    await connectorAddApiKey({ provider: "generic-rest", label: "My API", apiKey: "sekret-key" });
    expect(invokeMock).toHaveBeenCalledWith("connector_add_api_key", {
      provider: "generic-rest",
      label: "My API",
      apiKey: "sekret-key",
    });
  });

  it("connectorListAccounts sends no args", async () => {
    await connectorListAccounts();
    expect(invokeMock).toHaveBeenCalledWith("connector_list_accounts");
  });

  it("connectorDeleteAccount sends id", async () => {
    await connectorDeleteAccount({ id: "a1" });
    expect(invokeMock).toHaveBeenCalledWith("connector_delete_account", { id: "a1" });
  });

  it("connectorListOps sends accountId", async () => {
    await connectorListOps({ accountId: "a1" });
    expect(invokeMock).toHaveBeenCalledWith("connector_list_ops", { accountId: "a1" });
  });

  it("connectorInvoke sends accountId/op/argsJson/projectId, defaulting projectId to null", async () => {
    await connectorInvoke({ accountId: "a1", op: "get", argsJson: '{"path":"/x"}' });
    expect(invokeMock).toHaveBeenCalledWith("connector_invoke", {
      accountId: "a1",
      op: "get",
      argsJson: '{"path":"/x"}',
      projectId: null,
    });
  });

  it("connectorInvoke passes projectId through when given", async () => {
    await connectorInvoke({ accountId: "a1", op: "get", argsJson: "{}", projectId: "p1" });
    expect(invokeMock).toHaveBeenCalledWith("connector_invoke", {
      accountId: "a1",
      op: "get",
      argsJson: "{}",
      projectId: "p1",
    });
  });

  // ── Skills (S-EXT §5/§8, D11, Q14, T17) ──────────────────────────────────────────────────

  it("skillAdd sends name/description/mdPath/scope/projectId in T17's skill_add param order", async () => {
    await skillAdd("My Skill", "does things", "/tmp/skills/demo/SKILL.md", "global", null);
    expect(invokeMock).toHaveBeenCalledWith("skill_add", {
      name: "My Skill",
      description: "does things",
      mdPath: "/tmp/skills/demo/SKILL.md",
      scope: "global",
      projectId: null,
    });
  });

  it("skillAdd sends null name/description as-is (parsed from SKILL.md frontmatter on the orchd side)", async () => {
    await skillAdd(null, null, "/tmp/skills/demo/SKILL.md", "global", null);
    expect(invokeMock).toHaveBeenCalledWith("skill_add", {
      name: null,
      description: null,
      mdPath: "/tmp/skills/demo/SKILL.md",
      scope: "global",
      projectId: null,
    });
  });

  it("skillList sends projectId", async () => {
    await skillList(null);
    expect(invokeMock).toHaveBeenCalledWith("skill_list", { projectId: null });
  });

  it("skillDelete sends id", async () => {
    await skillDelete("s1");
    expect(invokeMock).toHaveBeenCalledWith("skill_delete", { id: "s1" });
  });

  // ── Trust: policy caps + audit log (S-EXT §4/§5/§6, BL-22, T18) ──────────────────────────

  it("trustSetPolicy sends scope/refId/spendCapUsd/ratePerMin in T18's trust_set_policy param order", async () => {
    await trustSetPolicy("server", "mcp-1", 10, 30);
    expect(invokeMock).toHaveBeenCalledWith("trust_set_policy", {
      scope: "server",
      refId: "mcp-1",
      spendCapUsd: 10,
      ratePerMin: 30,
    });
  });

  it("trustSetPolicy sends null refId/spendCapUsd/ratePerMin as-is (global scope, unlimited caps)", async () => {
    await trustSetPolicy("global", null, null, null);
    expect(invokeMock).toHaveBeenCalledWith("trust_set_policy", {
      scope: "global",
      refId: null,
      spendCapUsd: null,
      ratePerMin: null,
    });
  });

  it("trustListPolicies sends no args", async () => {
    await trustListPolicies();
    expect(invokeMock).toHaveBeenCalledWith("trust_list_policies");
  });

  it("trustListAudit sends limit", async () => {
    await trustListAudit(50);
    expect(invokeMock).toHaveBeenCalledWith("trust_list_audit", { limit: 50 });
  });

  it("trustListAudit sends null limit as-is (no cap)", async () => {
    await trustListAudit(null);
    expect(invokeMock).toHaveBeenCalledWith("trust_list_audit", { limit: null });
  });

  // ── describeOrchdError ────────────────────────────────────────────────────────────────────

  describe("describeOrchdError", () => {
    it("maps daemon/Invariant", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Invariant", message: "last workspace" }),
      ).toBe("недопустимая операция: last workspace");
    });

    it("maps daemon/Conflict", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Conflict", message: "id already exists" }),
      ).toBe("конфликт: id already exists");
    });

    it("maps daemon/NotFound (ignores message)", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "NotFound", message: "whatever" }),
      ).toBe("не найдено");
    });

    it("maps daemon/Validation", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Validation", message: "title required" }),
      ).toBe("неверные данные: title required");
    });

    it("maps daemon/Io", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Io", message: "disk full" }),
      ).toBe("ошибка сервиса: disk full");
    });

    it("maps daemon/Consent (S-EXT §8)", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Consent", message: "consent_required" }),
      ).toBe("требуется согласие на подключение: consent_required");
    });

    it("maps daemon/Policy (S-EXT §8)", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "Policy", message: "tool_disabled" }),
      ).toBe("запрещено политикой: tool_disabled");
    });

    it("maps disconnected", () => {
      expect(describeOrchdError({ kind: "disconnected" })).toBe("оркестратор недоступен");
    });

    it("maps incompatibleOrchd", () => {
      expect(
        describeOrchdError({ kind: "incompatibleOrchd", orchdMin: 2, orchdMax: 3 }),
      ).toBe("оркестратор недоступен");
    });

    it("falls back honestly for an unrecognized kind", () => {
      expect(describeOrchdError({ kind: "internal" })).toBe("неизвестная ошибка оркестратора");
    });

    it("falls back honestly for a non-CommandError rejection (e.g. a plain Error)", () => {
      expect(describeOrchdError(new Error("boom"))).toBe("неизвестная ошибка оркестратора");
      expect(describeOrchdError("boom")).toBe("неизвестная ошибка оркестратора");
      expect(describeOrchdError(undefined)).toBe("неизвестная ошибка оркестратора");
    });

    it("falls back to a generic daemon message when an unmapped code still carries a message", () => {
      expect(
        describeOrchdError({ kind: "daemon", code: "SomeFutureCode", message: "details" }),
      ).toBe("details");
    });
  });
});
