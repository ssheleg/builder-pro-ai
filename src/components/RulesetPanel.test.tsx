// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within, act } from "@testing-library/react";

const orchdGetRulesetMock = vi.fn();
const orchdUpsertRulesetMock = vi.fn();
const orchdAcknowledgeRuleFileMock = vi.fn();
const orchdRevealRulesFileMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../ipc/orchd", () => ({
  orchdGetRuleset: (...a: unknown[]) => orchdGetRulesetMock(...a),
  orchdUpsertRuleset: (...a: unknown[]) => orchdUpsertRulesetMock(...a),
  orchdAcknowledgeRuleFile: (...a: unknown[]) => orchdAcknowledgeRuleFileMock(...a),
  orchdRevealRulesFile: (...a: unknown[]) => orchdRevealRulesFileMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { RulesetPanel } from "./RulesetPanel";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { RuleFileState, RuleSetView, SupervisorConfig } from "../ipc/orchd-types";

/** Default CEO supervisor config (SCN-046): disabled/empty, the shape every fresh policy carries. */
function makeSupervisor(over: Partial<SupervisorConfig> = {}): SupervisorConfig {
  return {
    enabled: over.enabled ?? false,
    delegatedClasses: over.delegatedClasses ?? [],
    instruction: over.instruction ?? "",
    customRules: over.customRules ?? [],
  };
}

function makeView(over: {
  fileState?: RuleFileState;
  mdContent?: string | null;
  spendCapUsd?: number | null;
  approvalClasses?: string[];
  pathAllowlist?: string[];
  supervisor?: SupervisorConfig;
  scope?: "global" | "project";
  projectId?: string | null;
} = {}): RuleSetView {
  const fileState = over.fileState ?? "ok";
  const scope = over.scope ?? "global";
  return {
    rule: {
      id: scope === "project" ? "rule-p1" : "rule-global",
      scope,
      projectId: over.projectId ?? (scope === "project" ? "p1" : null),
      mdPath: "/Users/x/Library/Application Support/BuilderProAI/rules/global.md",
      mdHash: "hash-1",
      policy: {
        spendCapUsd: over.spendCapUsd ?? null,
        approvalClasses: over.approvalClasses ?? [],
        pathAllowlist: over.pathAllowlist ?? [],
        supervisor: over.supervisor ?? makeSupervisor(),
      },
      createdAt: 1,
      updatedAt: 1,
    },
    mdContent: over.mdContent !== undefined ? over.mdContent : fileState === "missing" ? null : "# rules\n",
    fileState,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdGetRulesetMock.mockReset().mockResolvedValue(makeView());
  orchdUpsertRulesetMock.mockReset().mockResolvedValue(makeView());
  orchdAcknowledgeRuleFileMock.mockReset().mockResolvedValue(makeView());
  orchdRevealRulesFileMock.mockReset().mockResolvedValue(undefined);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState({ rulesets: {}, toast: null, toastQueue: [], orchdDown: false }, false);
});

describe("RulesetPanel", () => {
  it('fileState "ok": renders an editable textarea bound to mdContent and no banner', () => {
    useAppStore.setState({ rulesets: { global: makeView({ fileState: "ok", mdContent: "# hi\n" }) } }, false);

    render(<RulesetPanel scope="global" projectId={null} />);

    const textarea = screen.getByTestId("ruleset-content") as HTMLTextAreaElement;
    expect(textarea.value).toBe("# hi\n");
    expect(textarea.disabled).toBe(false);
    expect(screen.queryByTestId("ruleset-banner-modified")).toBeNull();
    expect(screen.queryByTestId("ruleset-banner-missing")).toBeNull();
  });

  it('fileState "externallyModified": renders an info banner + [Accept] button', () => {
    useAppStore.setState(
      { rulesets: { global: makeView({ fileState: "externallyModified", mdContent: "# changed on disk\n" }) } },
      false,
    );

    render(<RulesetPanel scope="global" projectId={null} />);

    const banner = screen.getByTestId("ruleset-banner-modified");
    expect(within(banner).getByRole("button", { name: strings.common.accept })).toBeTruthy();
    expect(screen.queryByTestId("ruleset-banner-missing")).toBeNull();
  });

  it('fileState "missing": renders a banner "file lost" + [Recreate] button, no textarea', () => {
    useAppStore.setState({ rulesets: { global: makeView({ fileState: "missing", mdContent: null }) } }, false);

    render(<RulesetPanel scope="global" projectId={null} />);

    const banner = screen.getByTestId("ruleset-banner-missing");
    expect(within(banner).getByText(strings.rules.missingBanner)).toBeTruthy();
    expect(within(banner).getByRole("button", { name: strings.rules.recreate })).toBeTruthy();
    expect(screen.queryByTestId("ruleset-content")).toBeNull();
  });

  it("[Accept] calls orchdAcknowledgeRuleFile(rule.id) and refreshes the view", async () => {
    const view = makeView({ fileState: "externallyModified", mdContent: "# changed\n" });
    const acknowledged = makeView({ fileState: "ok", mdContent: "# changed\n" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    // The component's post-acknowledge refresh round-trips through `orchdGetRuleset` (via the
    // store's `refreshRuleset`), NOT the acknowledge call's own return value — both must reflect
    // the new "ok" state for the refreshed store entry to actually flip.
    orchdGetRulesetMock.mockResolvedValue(acknowledged);
    orchdAcknowledgeRuleFileMock.mockResolvedValue(acknowledged);

    render(<RulesetPanel scope="global" projectId={null} />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.accept }));

    await waitFor(() => expect(orchdAcknowledgeRuleFileMock).toHaveBeenCalledWith("rule-global"));
    await waitFor(() => expect(useAppStore.getState().rulesets["global"]?.fileState).toBe("ok"));
  });

  it("Save (content) calls orchdUpsertRuleset with the edited mdContent, null mdPath and null policy", async () => {
    const view = makeView({ fileState: "ok", mdContent: "old\n" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);
    const textarea = screen.getByTestId("ruleset-content");
    fireEvent.change(textarea, { target: { value: "new content\n" } });
    fireEvent.click(screen.getByTestId("ruleset-save-content"));

    await waitFor(() =>
      expect(orchdUpsertRulesetMock).toHaveBeenCalledWith("global", null, "new content\n", null, null),
    );
  });

  it('"Recreate" on a missing file calls orchdUpsertRuleset with mdContent: ""', async () => {
    const view = makeView({ fileState: "missing", mdContent: null });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);
    fireEvent.click(screen.getByRole("button", { name: strings.rules.recreate }));

    await waitFor(() => expect(orchdUpsertRulesetMock).toHaveBeenCalledWith("global", null, "", null, null));
  });

  it('"reveal file" calls orchdRevealRulesFile with scope+projectId only, never a path arg', async () => {
    const view = makeView({ fileState: "ok" });
    useAppStore.setState({ rulesets: { "project:p1": view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="project" projectId="p1" />);
    fireEvent.click(screen.getByRole("button", { name: strings.rules.revealFile }));

    await waitFor(() => expect(orchdRevealRulesFileMock).toHaveBeenCalledWith("project", "p1"));
    expect(orchdRevealRulesFileMock).toHaveBeenCalledWith(
      expect.not.stringMatching(/\//),
      expect.anything(),
    );
    // exactly two args — a stray third (path) argument would fail this exact-call assertion.
    expect(orchdRevealRulesFileMock.mock.calls[0]).toEqual(["project", "p1"]);
  });

  it("a negative spend cap is blocked client-side: orchdUpsertRuleset is NOT called and an inline message is shown", async () => {
    const view = makeView({ fileState: "ok" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);
    const spendCapInput = screen.getByTestId("ruleset-spend-cap");
    fireEvent.change(spendCapInput, { target: { value: "-5" } });
    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    expect(screen.getByTestId("ruleset-policy-error")).toBeTruthy();
    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
  });

  it("SEC-4: the inert spend-cap hint renders with the spend-cap control (stored cap ≠ enforced cap)", () => {
    const view = makeView({ fileState: "ok" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);

    const hint = screen.getByTestId("ruleset-spend-cap-inert-hint");
    expect(hint.textContent).toBe(strings.rules.spendCapInertHint);
    // Proximity: the hint follows the spend-cap input in the same policy editor, so the
    // disclaimer is read with the control whose enforcement it de-implies.
    const spendCap = screen.getByTestId("ruleset-spend-cap");
    expect(spendCap.compareDocumentPosition(hint) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("policy save: empty spend cap saves null (unlimited), and a valid non-negative cap round-trips", async () => {
    const view = makeView({ fileState: "ok", spendCapUsd: 10 });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);
    const spendCapInput = screen.getByTestId("ruleset-spend-cap") as HTMLInputElement;
    expect(spendCapInput.value).toBe("10");

    fireEvent.change(spendCapInput, { target: { value: "" } });
    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    await waitFor(() =>
      expect(orchdUpsertRulesetMock).toHaveBeenCalledWith("global", null, null, null, {
        spendCapUsd: null,
        approvalClasses: [],
        pathAllowlist: [],
        supervisor: makeSupervisor(),
      }),
    );
  });

  it("approval-classes and allowlist chip inputs add entries and include them in the saved policy", async () => {
    const view = makeView({ fileState: "ok" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);

    fireEvent.change(screen.getByTestId("ruleset-approval-class-input"), {
      target: { value: "deploy" },
    });
    fireEvent.click(screen.getByTestId("ruleset-approval-class-add"));

    fireEvent.change(screen.getByTestId("ruleset-allowlist-input"), {
      target: { value: "/repo/src" },
    });
    fireEvent.click(screen.getByTestId("ruleset-allowlist-add"));

    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    await waitFor(() =>
      expect(orchdUpsertRulesetMock).toHaveBeenCalledWith("global", null, null, null, {
        spendCapUsd: null,
        approvalClasses: ["deploy"],
        pathAllowlist: ["/repo/src"],
        supervisor: makeSupervisor(),
      }),
    );
  });

  it("a server Validation error from a policy save surfaces via showToast", async () => {
    const view = makeView({ fileState: "ok" });
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);
    const commandError = { kind: "daemon", code: "Validation", message: "invalid policy" };
    orchdUpsertRulesetMock.mockRejectedValueOnce(commandError);

    render(<RulesetPanel scope="global" projectId={null} />);
    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("orchestrator: error"));
  });

  it("mounts and refreshes: calls orchdGetRuleset(scope, projectId) on mount", async () => {
    const view = makeView();
    useAppStore.setState({ rulesets: { global: view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="global" projectId={null} />);

    await waitFor(() => expect(orchdGetRulesetMock).toHaveBeenCalledWith("global", null));
  });

  it("while orchdDown: every mutating (Save/Accept/Recreate) button is disabled and clicking one never calls the orchd wrapper (spec §10)", () => {
    const modified = makeView({ fileState: "externallyModified", mdContent: "# changed\n" });
    useAppStore.setState({ rulesets: { global: modified }, orchdDown: true }, false);

    render(<RulesetPanel scope="global" projectId={null} />);

    const saveContentButton = screen.getByTestId("ruleset-save-content") as HTMLButtonElement;
    const acknowledgeButton = screen.getByTestId("ruleset-acknowledge") as HTMLButtonElement;
    const savePolicyButton = screen.getByTestId("ruleset-save-policy") as HTMLButtonElement;

    expect(saveContentButton.disabled).toBe(true);
    expect(acknowledgeButton.disabled).toBe(true);
    expect(savePolicyButton.disabled).toBe(true);

    fireEvent.click(saveContentButton);
    fireEvent.click(acknowledgeButton);
    fireEvent.click(savePolicyButton);

    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
    expect(orchdAcknowledgeRuleFileMock).not.toHaveBeenCalled();
  });

  it('while orchdDown: the "Recreate" button (missing file state) is disabled', () => {
    const missing = makeView({ fileState: "missing", mdContent: null });
    useAppStore.setState({ rulesets: { global: missing }, orchdDown: true }, false);

    render(<RulesetPanel scope="global" projectId={null} />);

    const recreateButton = screen.getByTestId("ruleset-recreate") as HTMLButtonElement;
    expect(recreateButton.disabled).toBe(true);

    fireEvent.click(recreateButton);
    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
  });
});

// ── CEO supervisor section (SCN-046, FLW-19, A-7) ──────────────────────────────────────────────
describe("RulesetPanel — CEO supervisor (SCN-046)", () => {
  /** Set up a project-scoped view in the store AND as the mount-refresh return value, so the
   * on-mount re-Get doesn't clobber the fixture (same pattern as the "reveal file" test above). */
  function mountProject(over: Parameters<typeof makeView>[0] = {}) {
    const view = makeView({ ...over, scope: "project", projectId: "p1" });
    useAppStore.setState({ rulesets: { "project:p1": view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);
    return render(<RulesetPanel scope="project" projectId="p1" />);
  }

  it("progressive disclosure (PRN-11): while OFF only the toggle, disabled hint and pending note render — no scope/info summaries", () => {
    mountProject();

    expect(screen.getByTestId("ruleset-supervisor")).toBeTruthy();
    const enable = screen.getByTestId("ruleset-supervisor-enable") as HTMLInputElement;
    expect(enable.checked).toBe(false);
    // The muted "enable to configure" hint is the only detail shown while disabled.
    expect(screen.getByTestId("ruleset-supervisor-disabled-hint").textContent).toBe(
      strings.rules.supervisor.disabledHint,
    );
    // The detail controls / "active grant"-implying summaries are ABSENT while disabled.
    expect(screen.queryByTestId("ruleset-supervisor-info-access")).toBeNull();
    expect(screen.queryByTestId("ruleset-supervisor-scope-summary")).toBeNull();
    expect(screen.queryByTestId("ruleset-supervisor-mcp-soon")).toBeNull();
    expect(screen.queryByTestId("ruleset-supervisor-recommended")).toBeNull();
    expect(screen.queryByTestId("ruleset-supervisor-instruction")).toBeNull();
    expect(screen.queryByTestId("ruleset-supervisor-inherited-caps")).toBeNull();
    // Honesty boundary (S6b): the pending note is always present, verbatim, even while disabled.
    const pending = screen.getByTestId("ruleset-supervisor-pending");
    expect(pending.textContent).toBe(strings.rules.supervisor.pendingNote);
    expect(pending.textContent).toContain("S6b");
  });

  it("enabling the CEO reveals the detail controls (info-access, scope summary, MCP-soon) and hides the hint", () => {
    mountProject();

    fireEvent.click(screen.getByTestId("ruleset-supervisor-enable"));

    expect(screen.queryByTestId("ruleset-supervisor-disabled-hint")).toBeNull();
    expect(screen.getByTestId("ruleset-supervisor-info-access").textContent).toBe(
      strings.rules.supervisor.infoAccess,
    );
    // Scope summary with no classes and no cap reads the "no classes"/"no spend cap" fallbacks.
    expect(screen.getByTestId("ruleset-supervisor-scope-summary").textContent).toBe(
      strings.rules.supervisor.scopeSummary(
        strings.rules.supervisor.scopeSummaryNoClasses,
        strings.rules.supervisor.inheritedNoSpendCap,
      ),
    );
    expect(screen.getByTestId("ruleset-supervisor-mcp-soon").textContent).toBe(
      strings.rules.supervisor.mcpSoon,
    );
    // The pending note stays present alongside the revealed controls.
    expect(screen.getByTestId("ruleset-supervisor-pending").textContent).toBe(
      strings.rules.supervisor.pendingNote,
    );
  });

  it("does not render the supervisor section on the global rules view", () => {
    useAppStore.setState({ rulesets: { global: makeView() } }, false);
    render(<RulesetPanel scope="global" projectId={null} />);
    expect(screen.queryByTestId("ruleset-supervisor")).toBeNull();
  });

  it("enable + delegated class + instruction: Save sends the full supervisor config", async () => {
    mountProject();

    // Enable first (progressive disclosure — the detail controls only exist once enabled), then
    // seed the delegation scope via the Recommended-scope preset and write the instruction.
    fireEvent.click(screen.getByTestId("ruleset-supervisor-enable"));
    fireEvent.click(screen.getByTestId("ruleset-supervisor-recommended"));
    fireEvent.change(screen.getByTestId("ruleset-supervisor-instruction"), {
      target: { value: "Ship small." },
    });

    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    await waitFor(() =>
      expect(orchdUpsertRulesetMock).toHaveBeenCalledWith("project", "p1", null, null, {
        spendCapUsd: null,
        approvalClasses: [],
        pathAllowlist: [],
        supervisor: {
          enabled: true,
          delegatedClasses: ["safe-shell", "file-write"],
          instruction: "Ship small.",
          customRules: [],
        },
      }),
    );
  });

  it("Recommended scope seeds the safe-shell + file-write delegated classes as checked", () => {
    mountProject();

    // Enable to reveal the detail controls, then apply the preset.
    fireEvent.click(screen.getByTestId("ruleset-supervisor-enable"));
    fireEvent.click(screen.getByTestId("ruleset-supervisor-recommended"));

    const safeShell = screen.getByTestId("ruleset-supervisor-class-safe-shell") as HTMLInputElement;
    const fileWrite = screen.getByTestId("ruleset-supervisor-class-file-write") as HTMLInputElement;
    expect(safeShell.checked).toBe(true);
    expect(fileWrite.checked).toBe(true);
  });

  it("enabled CEO with an empty delegation scope: blocked alert shown, Save NOT sent", () => {
    mountProject();

    // No classes delegated; enable the CEO, then attempt Save.
    fireEvent.click(screen.getByTestId("ruleset-supervisor-enable"));
    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    const alert = screen.getByTestId("ruleset-policy-error");
    expect(alert.textContent).toBe(strings.rules.supervisor.blockedNoClasses);
    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
  });

  it("unchecking the last delegated class then Save re-blocks (guard is on the live scope, not stale)", async () => {
    mountProject({ supervisor: makeSupervisor({ enabled: true, delegatedClasses: ["safe-shell"] }) });

    // Starts valid (enabled + one class) — uncheck it, then Save must block.
    const safeShell = screen.getByTestId("ruleset-supervisor-class-safe-shell") as HTMLInputElement;
    expect(safeShell.checked).toBe(true);
    fireEvent.click(safeShell);
    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    expect(screen.getByTestId("ruleset-policy-error").textContent).toBe(
      strings.rules.supervisor.blockedNoClasses,
    );
    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
  });

  it("orchd down (PRN-04): CEO controls stay editable as drafts; only Save policy is gated", () => {
    const view = makeView({ scope: "project", projectId: "p1" });
    useAppStore.setState({ rulesets: { "project:p1": view }, orchdDown: true }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="project" projectId="p1" />);

    // The toggle stays live while orchd is down (unified drafts-stay-live rule, not disabled).
    const enable = screen.getByTestId("ruleset-supervisor-enable") as HTMLInputElement;
    expect(enable.disabled).toBe(false);
    // Enabling it reveals the now-live detail controls (progressive disclosure composes with the
    // unified gating): recommended, instruction, and the custom-rule adder are all editable.
    fireEvent.click(enable);
    expect(
      (screen.getByTestId("ruleset-supervisor-recommended") as HTMLButtonElement).disabled,
    ).toBe(false);
    expect(
      (screen.getByTestId("ruleset-supervisor-instruction") as HTMLTextAreaElement).disabled,
    ).toBe(false);
    expect((screen.getByTestId("ruleset-supervisor-rule-input") as HTMLInputElement).disabled).toBe(
      false,
    );
    // Only "Save policy" stays gated (the correctness gate that protects the write).
    const savePolicy = screen.getByTestId("ruleset-save-policy") as HTMLButtonElement;
    expect(savePolicy.disabled).toBe(true);
    fireEvent.click(savePolicy);
    expect(orchdUpsertRulesetMock).not.toHaveBeenCalled();
  });

  it("a server Validation reject on a supervisor save surfaces via toast (honest error surface)", async () => {
    mountProject({ supervisor: makeSupervisor({ enabled: true, delegatedClasses: ["safe-shell"] }) });
    const commandError = { kind: "daemon", code: "Validation", message: "invalid policy" };
    orchdUpsertRulesetMock.mockRejectedValueOnce(commandError);

    fireEvent.click(screen.getByTestId("ruleset-save-policy"));

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("orchestrator: error"));
  });
});

// ── Dirty-draft guard (PRN-03) ─────────────────────────────────────────────────────────────────
// A fresh view landing mid-edit (an `orchd://ruleset-changed` push or reconnect rehydrate) must
// NOT clobber an in-progress unsaved policy draft when the ruleset identity is unchanged; a clean
// field still re-hydrates, and navigating to a different ruleset always hydrates fully.
describe("RulesetPanel — dirty-draft guard (PRN-03)", () => {
  it("same-identity external update PRESERVES a dirty spend-cap draft", async () => {
    const view = makeView({ scope: "project", projectId: "p1", spendCapUsd: 10 });
    useAppStore.setState({ rulesets: { "project:p1": view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="project" projectId="p1" />);
    // Let the on-mount refresh settle so the baseline is the server's "10".
    await waitFor(() => expect(orchdGetRulesetMock).toHaveBeenCalled());
    const cap = () => screen.getByTestId("ruleset-spend-cap") as HTMLInputElement;
    await waitFor(() => expect(cap().value).toBe("10"));

    // User edits the cap (dirty), then a push lands for the SAME ruleset with a different value.
    fireEvent.change(cap(), { target: { value: "99" } });
    const external = makeView({ scope: "project", projectId: "p1", spendCapUsd: 20 });
    act(() => {
      useAppStore.setState({ rulesets: { "project:p1": external } }, false);
    });

    // The dirty draft survives — the push does not overwrite it.
    expect(cap().value).toBe("99");
  });

  it("same-identity external update HYDRATES a clean spend-cap draft", async () => {
    const view = makeView({ scope: "project", projectId: "p1", spendCapUsd: 10 });
    useAppStore.setState({ rulesets: { "project:p1": view } }, false);
    orchdGetRulesetMock.mockResolvedValue(view);

    render(<RulesetPanel scope="project" projectId="p1" />);
    const cap = () => screen.getByTestId("ruleset-spend-cap") as HTMLInputElement;
    await waitFor(() => expect(cap().value).toBe("10"));

    // No local edit (clean) — a push for the same ruleset re-hydrates to the server's new value.
    const external = makeView({ scope: "project", projectId: "p1", spendCapUsd: 20 });
    act(() => {
      useAppStore.setState({ rulesets: { "project:p1": external } }, false);
    });

    expect(cap().value).toBe("20");
  });

  it("switching to a different ruleset always hydrates (navigation, not clobber)", async () => {
    const p1 = makeView({ scope: "project", projectId: "p1", spendCapUsd: 10 });
    const p2 = makeView({ scope: "project", projectId: "p2", spendCapUsd: 55 });
    useAppStore.setState(
      { rulesets: { "project:p1": p1, "project:p2": p2 } },
      false,
    );
    orchdGetRulesetMock.mockImplementation((_scope: unknown, pid: unknown) =>
      Promise.resolve(pid === "p2" ? p2 : p1),
    );

    const { rerender } = render(<RulesetPanel scope="project" projectId="p1" />);
    const cap = () => screen.getByTestId("ruleset-spend-cap") as HTMLInputElement;
    await waitFor(() => expect(cap().value).toBe("10"));

    // Dirty p1's draft, then navigate to p2 — the identity change must hydrate p2's value fully,
    // discarding the p1 draft (this is navigation, not an underneath-edit clobber).
    fireEvent.change(cap(), { target: { value: "99" } });
    rerender(<RulesetPanel scope="project" projectId="p2" />);
    await waitFor(() => expect(cap().value).toBe("55"));
  });
});
