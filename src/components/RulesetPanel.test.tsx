// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within } from "@testing-library/react";

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
import type { RuleFileState, RuleSetView } from "../ipc/orchd-types";

function makeView(over: {
  fileState?: RuleFileState;
  mdContent?: string | null;
  spendCapUsd?: number | null;
  approvalClasses?: string[];
  pathAllowlist?: string[];
} = {}): RuleSetView {
  const fileState = over.fileState ?? "ok";
  return {
    rule: {
      id: "rule-global",
      scope: "global",
      projectId: null,
      mdPath: "/Users/x/Library/Application Support/BuilderProAI/rules/global.md",
      mdHash: "hash-1",
      policy: {
        spendCapUsd: over.spendCapUsd ?? null,
        approvalClasses: over.approvalClasses ?? [],
        pathAllowlist: over.pathAllowlist ?? [],
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
