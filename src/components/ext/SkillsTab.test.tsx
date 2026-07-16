// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const skillAddMock = vi.fn();
const skillDeleteMock = vi.fn();
// `refreshSkills` (store.ts) calls `skillList` straight through — mocked here too (same module)
// so SkillsTab's mount-time fetch resolves deterministically, mirroring
// ConnectorsTab.test.tsx's relationship with `connectorListAccounts`/`refreshAccounts`.
const skillListMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../../ipc/orchd", () => ({
  skillAdd: (...a: unknown[]) => skillAddMock(...a),
  skillDelete: (...a: unknown[]) => skillDeleteMock(...a),
  skillList: (...a: unknown[]) => skillListMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

const pickSkillFileMock = vi.fn();
vi.mock("../../ipc/commands", () => ({
  pickSkillFile: (...a: unknown[]) => pickSkillFileMock(...a),
}));

import { SkillsTab } from "./SkillsTab";
import { useAppStore } from "../../store/store";
import type { Skill } from "../../ipc/orchd-types";
import { strings } from "../../strings";

function makeSkill(over: Partial<Skill> = {}): Skill {
  return {
    id: "s1",
    name: "My Skill",
    description: "does a thing",
    mdPath: "/Users/demo/skills/my-skill/SKILL.md",
    mdHash: "deadbeef",
    scope: "global",
    projectId: null,
    fileState: "present",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);
beforeEach(() => {
  skillAddMock.mockReset().mockResolvedValue(makeSkill());
  skillDeleteMock.mockReset().mockResolvedValue(undefined);
  skillListMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  pickSkillFileMock.mockReset().mockResolvedValue("/Users/demo/skills/my-skill/SKILL.md");
  vi.spyOn(window, "confirm").mockReturnValue(true);
  useAppStore.setState({ skills: [], orchdDown: false }, false);
});

describe("SkillsTab", () => {
  it("fetches skills (refreshSkills -> skillList) on mount", async () => {
    render(<SkillsTab />);
    await waitFor(() => {
      expect(skillListMock).toHaveBeenCalledWith(null);
    });
  });

  it("renders the plumbing-only banner (D11: no runtime consumer until S6b)", () => {
    render(<SkillsTab />);
    const banner = screen.getByTestId("skills-banner");
    expect(banner.textContent).toContain("registry");
    expect(banner.textContent).toContain("S6b");
  });

  it("renders a stubbed skills list (name, description, path, scope)", () => {
    useAppStore.setState(
      {
        skills: [
          makeSkill({
            id: "s1",
            name: "Research Helper",
            description: "helps research",
            mdPath: "/Users/demo/skills/research/SKILL.md",
            scope: "global",
          }),
        ],
      },
      false,
    );
    render(<SkillsTab />);
    expect(screen.getByTestId("skill-row-s1")).toBeTruthy();
    expect(screen.getByTestId("skill-name-s1").textContent).toBe("Research Helper");
    expect(screen.getByTestId("skill-description-s1").textContent).toBe("helps research");
    expect(screen.getByTestId("skill-path-s1").textContent).toBe(
      "/Users/demo/skills/research/SKILL.md",
    );
    expect(screen.getByTestId("skill-scope-s1").textContent).toBe(strings.common.scope.global);
    expect(screen.queryByTestId("skills-empty")).toBeNull();
  });

  it("renders an empty-state message when there are no skills", () => {
    render(<SkillsTab />);
    expect(screen.getByTestId("skills-empty")).toBeTruthy();
  });

  // ---- files-as-truth badge ----

  it("a present-state skill renders no file-state badge", () => {
    useAppStore.setState({ skills: [makeSkill({ fileState: "present" })] }, false);
    render(<SkillsTab />);
    expect(screen.queryByTestId("skill-filestate-s1")).toBeNull();
  });

  it("a modified-state skill renders the \"modified\" badge", () => {
    useAppStore.setState({ skills: [makeSkill({ fileState: "modified" })] }, false);
    render(<SkillsTab />);
    expect(screen.getByTestId("skill-filestate-s1").textContent).toBe(
      strings.ext.skills.badge.modified,
    );
  });

  it("a missing-state skill renders the \"file missing\" badge", () => {
    useAppStore.setState({ skills: [makeSkill({ fileState: "missing" })] }, false);
    render(<SkillsTab />);
    expect(screen.getByTestId("skill-filestate-s1").textContent).toBe(
      strings.ext.skills.badge.missing,
    );
  });

  // ---- add (pick SKILL.md -> skillAdd) ----

  it("\"choose SKILL.md\" calls pickSkillFile and shows the picked path", async () => {
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => {
      expect(pickSkillFileMock).toHaveBeenCalledWith();
    });
    await waitFor(() => {
      expect(screen.getByTestId("skill-picked-path").textContent).toBe(
        "/Users/demo/skills/my-skill/SKILL.md",
      );
    });
  });

  it("cancelling the file picker (null) leaves no path picked and the submit disabled", async () => {
    pickSkillFileMock.mockResolvedValue(null);
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => {
      expect(pickSkillFileMock).toHaveBeenCalled();
    });
    expect(screen.queryByTestId("skill-picked-path")).toBeNull();
    expect(screen.getByTestId("skill-create-submit")).toHaveProperty("disabled", true);
  });

  it("submit stays disabled until a SKILL.md path is picked", async () => {
    render(<SkillsTab />);
    expect(screen.getByTestId("skill-create-submit")).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => {
      expect(screen.getByTestId("skill-create-submit")).toHaveProperty("disabled", false);
    });
  });

  it("submit calls skillAdd with name/description trimmed to null when empty, path, scope global, project null; then re-fetches and clears the form", async () => {
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => screen.getByTestId("skill-picked-path"));

    fireEvent.click(screen.getByTestId("skill-create-submit"));
    await waitFor(() => {
      expect(skillAddMock).toHaveBeenCalledWith(
        null,
        null,
        "/Users/demo/skills/my-skill/SKILL.md",
        "global",
        null,
      );
    });
    await waitFor(() => {
      expect(skillListMock).toHaveBeenCalledTimes(2); // mount + post-add refresh
    });
    // form clears after a successful add
    expect(screen.queryByTestId("skill-picked-path")).toBeNull();
  });

  it("two rapid '+ skill' clicks add ONCE (double-submit guard, spec D6 / P-19)", async () => {
    let resolveAdd!: (v: unknown) => void;
    skillAddMock.mockReset().mockImplementation(() => new Promise((res) => (resolveAdd = res)));
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => screen.getByTestId("skill-picked-path"));

    const submit = screen.getByTestId("skill-create-submit");
    fireEvent.click(submit);
    fireEvent.click(submit);

    expect(skillAddMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveAdd({ id: "sk9" });
    });
  });

  it("submit passes trimmed name/description when provided", async () => {
    render(<SkillsTab />);
    fireEvent.change(screen.getByTestId("skill-create-name"), {
      target: { value: "  Custom Name  " },
    });
    fireEvent.change(screen.getByTestId("skill-create-description"), {
      target: { value: "  a description  " },
    });
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => screen.getByTestId("skill-picked-path"));
    fireEvent.click(screen.getByTestId("skill-create-submit"));

    await waitFor(() => {
      expect(skillAddMock).toHaveBeenCalledWith(
        "Custom Name",
        "a description",
        "/Users/demo/skills/my-skill/SKILL.md",
        "global",
        null,
      );
    });
  });

  it("a failed skillAdd shows a toast and leaves the picked path intact", async () => {
    skillAddMock.mockRejectedValue(new Error("boom"));
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => screen.getByTestId("skill-picked-path"));
    fireEvent.click(screen.getByTestId("skill-create-submit"));

    await waitFor(() => {
      expect(describeOrchdErrorMock).toHaveBeenCalled();
    });
    expect(screen.getByTestId("skill-picked-path")).toBeTruthy();
  });

  it("a failed file picker keeps the sessiond CommandError message via describeCommandError, not the generic orchd fallback (P-16)", async () => {
    useAppStore.setState({ toast: null, toastQueue: [] }, false);
    pickSkillFileMock.mockReset().mockRejectedValueOnce({ kind: "internal", message: "native picker crashed" });
    render(<SkillsTab />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("skill-pick-path"));
    });
    // The real (unmocked) `describeCommandError` preserves the sessiond message …
    expect(useAppStore.getState().toast).toBe("native picker crashed");
    // … and the orchd-specific mapper (which would flatten it to "unknown orchestrator error") is
    // NOT on the picker path anymore.
    expect(describeOrchdErrorMock).not.toHaveBeenCalled();
  });

  // ---- delete ----

  it("clicking delete confirms then calls skillDelete", async () => {
    skillListMock.mockResolvedValue([makeSkill()]);
    useAppStore.setState({ skills: [makeSkill()] }, false);
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-delete-s1"));
    await waitFor(() => {
      expect(skillDeleteMock).toHaveBeenCalledWith("s1");
    });
  });

  it("declining the confirm dialog skips skillDelete", () => {
    (window.confirm as ReturnType<typeof vi.fn>).mockReturnValue(false);
    skillListMock.mockResolvedValue([makeSkill()]);
    useAppStore.setState({ skills: [makeSkill()] }, false);
    render(<SkillsTab />);
    fireEvent.click(screen.getByTestId("skill-delete-s1"));
    expect(skillDeleteMock).not.toHaveBeenCalled();
  });

  // ---- honest degradation ----

  it("orchdDown:true disables the submit and delete controls, and clicking each never calls its wrapper (ServersTab/ConnectorsTab precedent)", async () => {
    skillListMock.mockResolvedValue([makeSkill()]);
    useAppStore.setState({ skills: [makeSkill()], orchdDown: false }, false);
    render(<SkillsTab />);

    // Populate every field with orchdDown:false FIRST so each control's OWN disable-condition
    // (no path picked yet) is already satisfied — the later assertion is then provably owed to
    // orchdDown ALONE (mirrors ConnectorsTab.test.tsx's pattern).
    fireEvent.click(screen.getByTestId("skill-pick-path"));
    await waitFor(() => screen.getByTestId("skill-picked-path"));

    act(() => useAppStore.setState({ orchdDown: true }, false));

    const controls = [
      screen.getByTestId("skill-create-submit"),
      screen.getByTestId("skill-delete-s1"),
    ];
    for (const c of controls) expect(c).toHaveProperty("disabled", true);

    // `user.click` faithfully emulates a real user click, which the browser suppresses on a
    // disabled control (plain `fireEvent.click` does not gate on `disabled` in jsdom).
    const user = userEvent.setup();
    for (const c of controls) await user.click(c);

    expect(skillAddMock).not.toHaveBeenCalled();
    expect(skillDeleteMock).not.toHaveBeenCalled();
  });
});
