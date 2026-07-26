// @vitest-environment jsdom
// SCN-054 (FLW-21, ST-041): the Docs tab — list + create ("+ doc", empty name blocked), the
// textarea/preview editor, Save (reject → inline + toast, content preserved), the
// Accept/Recreate file-state banners (the SCN-036 rules pattern), delete behind the locked
// "delete document?" confirm, "reveal file" (never a path from JS), and orchd-down gating.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within, act } from "@testing-library/react";

const orchdListDocsMock = vi.fn();
const orchdGetDocMock = vi.fn();
const orchdUpsertDocMock = vi.fn();
const orchdDeleteDocMock = vi.fn();
const orchdAcknowledgeDocFileMock = vi.fn();
const orchdRevealDocFileMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
const isNotFoundErrorMock = vi.fn((..._a: unknown[]) => false);

vi.mock("../ipc/orchd", () => ({
  orchdListDocs: (...a: unknown[]) => orchdListDocsMock(...a),
  orchdGetDoc: (...a: unknown[]) => orchdGetDocMock(...a),
  orchdUpsertDoc: (...a: unknown[]) => orchdUpsertDocMock(...a),
  orchdDeleteDoc: (...a: unknown[]) => orchdDeleteDocMock(...a),
  orchdAcknowledgeDocFile: (...a: unknown[]) => orchdAcknowledgeDocFileMock(...a),
  orchdRevealDocFile: (...a: unknown[]) => orchdRevealDocFileMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
  isNotFoundError: (...a: unknown[]) => isNotFoundErrorMock(...a),
}));

import { DocsPanel, formatRelativeTime, validateDocNameDraft } from "./DocsPanel";
import { useAppStore, docViewKey } from "../store/store";
import { strings } from "../strings";
import type { DocMeta, DocView, RuleFileState } from "../ipc/orchd-types";

function makeMeta(name: string, modifiedAt = 1_700_000_000_000): DocMeta {
  return { name, modifiedAt };
}

function makeView(over: {
  name?: string;
  fileState?: RuleFileState;
  mdContent?: string | null;
} = {}): DocView {
  const fileState = over.fileState ?? "ok";
  const name = over.name ?? "notes";
  return {
    doc: {
      id: `doc-${name}`,
      projectId: "p1",
      name,
      mdPath: `/app-support/rules/docs/p1/${name}.md`,
      mdHash: "hash-1",
      createdAt: 1,
      updatedAt: 1,
    },
    mdContent:
      over.mdContent !== undefined ? over.mdContent : fileState === "missing" ? null : "# doc\n",
    fileState,
  };
}

/** Store state with `notes` already listed AND selected-view preloaded under its key. */
function seedDoc(view: DocView): void {
  useAppStore.setState(
    {
      docsByProject: { p1: [makeMeta(view.doc.name)] },
      docViews: { [docViewKey("p1", view.doc.name)]: view },
    },
    false,
  );
}

afterEach(cleanup);

beforeEach(() => {
  orchdListDocsMock.mockReset().mockResolvedValue([]);
  orchdGetDocMock.mockReset().mockResolvedValue(makeView());
  orchdUpsertDocMock.mockReset().mockResolvedValue(makeView());
  orchdDeleteDocMock.mockReset().mockResolvedValue(undefined);
  orchdAcknowledgeDocFileMock.mockReset().mockResolvedValue(makeView());
  orchdRevealDocFileMock.mockReset().mockResolvedValue(undefined);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  isNotFoundErrorMock.mockReset().mockReturnValue(false);
  useAppStore.setState(
    { docsByProject: {}, docViews: {}, toast: null, toastQueue: [], orchdDown: false },
    false,
  );
});

describe("DocsPanel", () => {
  it("shows the loading state until the list is fetched, then the locked empty state", async () => {
    render(<DocsPanel projectId="p1" />);

    // `docsByProject.p1` is absent on first paint — the loading state.
    expect(screen.getByTestId("docs-loading").textContent).toBe(strings.docs.loading);

    // The mount refresh resolves to an empty list → SCN-054's locked empty-state copy.
    await waitFor(() => expect(screen.getByTestId("docs-empty")).toBeTruthy());
    expect(screen.getByTestId("docs-empty").textContent).toBe(strings.docs.empty);
    expect(orchdListDocsMock).toHaveBeenCalledWith("p1");
  });

  it("renders list rows with name + relative last-modified", async () => {
    const now = Date.now();
    orchdListDocsMock.mockResolvedValue([
      makeMeta("api-spec", now - 5 * 60_000),
      makeMeta("notes", now - 1000),
    ]);

    render(<DocsPanel projectId="p1" />);

    await waitFor(() => expect(screen.getByTestId("docs-list")).toBeTruthy());
    const spec = screen.getByTestId("docs-row-api-spec");
    expect(spec.textContent).toContain("api-spec");
    expect(spec.textContent).toContain(strings.docs.minutesAgo(5));
    expect(screen.getByTestId("docs-row-notes").textContent).toContain(strings.docs.justNow);
  });

  it('"+ doc" is disabled while the name is empty (SCN-054: empty name blocked)', () => {
    useAppStore.setState({ docsByProject: { p1: [] } }, false);

    render(<DocsPanel projectId="p1" />);

    expect((screen.getByTestId("docs-add") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "notes" } });
    expect((screen.getByTestId("docs-add") as HTMLButtonElement).disabled).toBe(false);
  });

  it('"+ doc" with a valid name creates via orchdUpsertDoc(projectId, name, "") and selects it', async () => {
    useAppStore.setState({ docsByProject: { p1: [] } }, false);
    orchdUpsertDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "" }));
    orchdListDocsMock.mockResolvedValue([makeMeta("notes")]);

    render(<DocsPanel projectId="p1" />);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "notes" } });
    fireEvent.click(screen.getByTestId("docs-add"));

    await waitFor(() => expect(orchdUpsertDocMock).toHaveBeenCalledWith("p1", "notes", ""));
    // The new doc opens in the editor and the name input clears for the next create.
    await waitFor(() => expect(screen.getByTestId("docs-content")).toBeTruthy());
    expect((screen.getByTestId("docs-add-name") as HTMLInputElement).value).toBe("");
  });

  it("an invalid (non-empty) name shows the inline mirror error and never round-trips", async () => {
    useAppStore.setState({ docsByProject: { p1: [] } }, false);

    render(<DocsPanel projectId="p1" />);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "../escape" } });
    fireEvent.click(screen.getByTestId("docs-add"));

    await waitFor(() =>
      expect(screen.getByTestId("docs-name-error").textContent).toBe(strings.docs.invalidName),
    );
    expect(orchdUpsertDocMock).not.toHaveBeenCalled();
  });

  it('"+ doc" over an existing name is blocked inline (upsert would blank that doc\'s file)', async () => {
    useAppStore.setState({ docsByProject: { p1: [makeMeta("notes")] } }, false);

    render(<DocsPanel projectId="p1" />);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "notes" } });
    fireEvent.click(screen.getByTestId("docs-add"));

    await waitFor(() =>
      expect(screen.getByTestId("docs-name-error").textContent).toBe(strings.docs.duplicateName),
    );
    expect(orchdUpsertDocMock).not.toHaveBeenCalled();
  });

  it("selecting a doc fetches it fresh and binds the textarea to its content", async () => {
    useAppStore.setState({ docsByProject: { p1: [makeMeta("notes")] } }, false);
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "# hello\n" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));

    await waitFor(() => expect(orchdGetDocMock).toHaveBeenCalledWith("p1", "notes"));
    await waitFor(() => {
      const textarea = screen.getByTestId("docs-content") as HTMLTextAreaElement;
      expect(textarea.value).toBe("# hello\n");
    });
    expect(screen.queryByTestId("docs-banner-modified")).toBeNull();
    expect(screen.queryByTestId("docs-banner-missing")).toBeNull();
  });

  it("Save sends the edited draft through orchdUpsertDoc", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "old\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "old\n" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-content")).toBeTruthy());

    fireEvent.change(screen.getByTestId("docs-content"), { target: { value: "new content\n" } });
    fireEvent.click(screen.getByTestId("docs-save"));

    await waitFor(() =>
      expect(orchdUpsertDocMock).toHaveBeenCalledWith("p1", "notes", "new content\n"),
    );
  });

  // ---- double-submit guards (FE-4, spec D6): one test per guard instance ----

  it('a rapid double-click on "+ doc" fires the create only once (create guard)', async () => {
    useAppStore.setState({ docsByProject: { p1: [] } }, false);
    // A never-settling upsert keeps the guard's ref lock engaged across both clicks.
    orchdUpsertDocMock.mockReset().mockReturnValue(new Promise<never>(() => {}));

    render(<DocsPanel projectId="p1" />);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "notes" } });
    const addBtn = screen.getByTestId("docs-add") as HTMLButtonElement;
    fireEvent.click(addBtn);
    fireEvent.click(addBtn);

    await waitFor(() => expect(orchdUpsertDocMock).toHaveBeenCalledTimes(1));
    expect(addBtn.disabled).toBe(true); // the visible affordance while the create is in flight
  });

  it("a rapid double-click on Save fires the upsert only once (editor guard)", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "old\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "old\n" }));
    // A never-settling upsert keeps the guard's ref lock engaged across both clicks.
    orchdUpsertDocMock.mockReset().mockReturnValue(new Promise<never>(() => {}));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-content")).toBeTruthy());

    const saveBtn = screen.getByTestId("docs-save") as HTMLButtonElement;
    fireEvent.click(saveBtn);
    fireEvent.click(saveBtn);

    await waitFor(() => expect(orchdUpsertDocMock).toHaveBeenCalledTimes(1));
    expect(saveBtn.disabled).toBe(true); // the visible affordance while the save is in flight
  });

  it("a rejected Save surfaces inline + toast and PRESERVES the editor content (SCN-054)", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "old\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "old\n" }));
    orchdUpsertDocMock.mockRejectedValue({ kind: "daemon", code: "Io", message: "disk full" });

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-content")).toBeTruthy());

    fireEvent.change(screen.getByTestId("docs-content"), { target: { value: "precious draft\n" } });
    fireEvent.click(screen.getByTestId("docs-save"));

    await waitFor(() =>
      expect(screen.getByTestId("docs-save-error").textContent).toBe("orchestrator: error"),
    );
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
    // The draft is verbatim — never discarded on a rejection.
    expect((screen.getByTestId("docs-content") as HTMLTextAreaElement).value).toBe(
      "precious draft\n",
    );
  });

  it('fileState "externallyModified": banner + [Accept] → orchdAcknowledgeDocFile(doc.id)', async () => {
    const modified = makeView({
      name: "notes",
      fileState: "externallyModified",
      mdContent: "# changed on disk\n",
    });
    seedDoc(modified);
    orchdGetDocMock.mockResolvedValue(modified);

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));

    await waitFor(() => expect(screen.getByTestId("docs-banner-modified")).toBeTruthy());
    const banner = screen.getByTestId("docs-banner-modified");
    expect(within(banner).getByText(strings.docs.modifiedBanner)).toBeTruthy();

    const accepted = makeView({ name: "notes", fileState: "ok", mdContent: "# changed on disk\n" });
    orchdAcknowledgeDocFileMock.mockResolvedValue(accepted);
    orchdGetDocMock.mockResolvedValue(accepted);
    fireEvent.click(screen.getByTestId("docs-acknowledge"));

    await waitFor(() => expect(orchdAcknowledgeDocFileMock).toHaveBeenCalledWith("doc-notes"));
    await waitFor(() => expect(screen.queryByTestId("docs-banner-modified")).toBeNull());
  });

  it('fileState "missing": "file lost" + [Recreate] → orchdUpsertDoc(projectId, name, ""), no textarea', async () => {
    const lost = makeView({ name: "notes", fileState: "missing", mdContent: null });
    seedDoc(lost);
    orchdGetDocMock.mockResolvedValue(lost);

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));

    await waitFor(() => expect(screen.getByTestId("docs-banner-missing")).toBeTruthy());
    const banner = screen.getByTestId("docs-banner-missing");
    expect(within(banner).getByText(strings.docs.missingBanner)).toBeTruthy();
    // No content to bind — the textarea (and Save) are absent, exactly the rules pattern.
    expect(screen.queryByTestId("docs-content")).toBeNull();
    expect(screen.queryByTestId("docs-save")).toBeNull();

    fireEvent.click(screen.getByTestId("docs-recreate"));
    await waitFor(() => expect(orchdUpsertDocMock).toHaveBeenCalledWith("p1", "notes", ""));
  });

  it('Delete asks the locked "delete document?" confirm and deletes only on OK', async () => {
    seedDoc(makeView({ name: "notes" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes" }));
    const confirmSpy = vi.spyOn(window, "confirm");

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-delete")).toBeTruthy());

    // Cancelled → no round-trip.
    confirmSpy.mockReturnValueOnce(false);
    fireEvent.click(screen.getByTestId("docs-delete"));
    expect(confirmSpy).toHaveBeenCalledWith(strings.docs.deleteConfirm);
    expect(orchdDeleteDocMock).not.toHaveBeenCalled();

    // FE-4: the cancelled click still took the submit guard's lock for its synchronous early
    // return — flush the microtask that releases it before the NEXT user click (in a real UI the
    // confirm dialog itself provides that gap between the two attempts).
    await act(async () => {});

    // Confirmed → DeleteDoc by the daemon-issued id.
    confirmSpy.mockReturnValueOnce(true);
    fireEvent.click(screen.getByTestId("docs-delete"));
    await waitFor(() => expect(orchdDeleteDocMock).toHaveBeenCalledWith("doc-notes"));
    confirmSpy.mockRestore();
  });

  it('"reveal file" calls orchdRevealDocFile with projectId+name only, never a path arg', async () => {
    seedDoc(makeView({ name: "notes" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-reveal")).toBeTruthy());
    fireEvent.click(screen.getByTestId("docs-reveal"));

    await waitFor(() => expect(orchdRevealDocFileMock).toHaveBeenCalledWith("p1", "notes"));
    expect(orchdRevealDocFileMock.mock.calls[0]).toHaveLength(2);
  });

  it("preview mode renders markdown blocks instead of the raw source", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "# Title\n- item one\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "# Title\n- item one\n" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-content")).toBeTruthy());

    fireEvent.click(screen.getByRole("radio", { name: strings.docs.modePreview }));

    const preview = screen.getByTestId("docs-preview");
    expect(screen.queryByTestId("docs-content")).toBeNull();
    // The heading renders as a heading element (level 1), not the literal "# Title" source line.
    expect(within(preview).getByRole("heading", { level: 1 }).textContent).toBe("Title");
    expect(within(preview).getByRole("listitem").textContent).toBe("item one");

    // Toggling back restores the editable textarea with the same draft.
    fireEvent.click(screen.getByRole("radio", { name: strings.docs.modeEdit }));
    expect((screen.getByTestId("docs-content") as HTMLTextAreaElement).value).toBe(
      "# Title\n- item one\n",
    );
  });

  it("orchd down: +doc/Save/Delete/Accept/Recreate disabled; reading, toggle and reveal stay live", async () => {
    const modified = makeView({ name: "notes", fileState: "externallyModified" });
    seedDoc(modified);
    orchdGetDocMock.mockResolvedValue(modified);
    useAppStore.setState({ orchdDown: true }, false);

    render(<DocsPanel projectId="p1" />);
    fireEvent.change(screen.getByTestId("docs-add-name"), { target: { value: "x" } });
    expect((screen.getByTestId("docs-add") as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByTestId("docs-row-notes"));
    await waitFor(() => expect(screen.getByTestId("docs-save")).toBeTruthy());

    expect((screen.getByTestId("docs-save") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("docs-delete") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("docs-acknowledge") as HTMLButtonElement).disabled).toBe(true);
    // Reading stays live: content is visible and editable locally, the mode toggle works, and
    // reveal (a local Finder action off a daemon read) is not orchd-gated.
    expect((screen.getByTestId("docs-content") as HTMLTextAreaElement).disabled).toBe(false);
    expect((screen.getByTestId("docs-reveal") as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("radio", { name: strings.docs.modePreview }));
    expect(screen.getByTestId("docs-preview")).toBeTruthy();

    // The missing-state Recreate is gated the same way.
    const lost = makeView({ name: "notes", fileState: "missing", mdContent: null });
    useAppStore.setState({ docViews: { [docViewKey("p1", "notes")]: lost } }, false);
    await waitFor(() =>
      expect((screen.getByTestId("docs-recreate") as HTMLButtonElement).disabled).toBe(true),
    );
  });

  it("no selection: the editor column shows the select prompt", () => {
    useAppStore.setState({ docsByProject: { p1: [makeMeta("notes")] } }, false);

    render(<DocsPanel projectId="p1" />);

    expect(screen.getByTestId("docs-select-prompt").textContent).toBe(strings.docs.selectPrompt);
  });
});

// ── Dirty-draft guard (PRN-03) ─────────────────────────────────────────────────────────────────
// A fresh doc view landing mid-edit (an `orchd://docs-changed` push or reconnect rehydrate) must
// NOT wipe an in-progress unsaved editor draft for the SAME doc; a clean editor still re-hydrates,
// and selecting a different doc always hydrates fully. The "file changed externally" banner
// mediates the same-doc conflict.
describe("DocsPanel — dirty-draft guard (PRN-03)", () => {
  it("a same-doc external change PRESERVES a dirty editor draft (banner mediates)", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "old\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "old\n" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    const editor = () => screen.getByTestId("docs-content") as HTMLTextAreaElement;
    await waitFor(() => expect(editor().value).toBe("old\n"));

    // User edits (dirty), then the same doc changes on disk (externallyModified push).
    fireEvent.change(editor(), { target: { value: "my unsaved edit\n" } });
    const external = makeView({
      name: "notes",
      fileState: "externallyModified",
      mdContent: "changed on disk\n",
    });
    act(() => {
      useAppStore.setState({ docViews: { [docViewKey("p1", "notes")]: external } }, false);
    });

    // The draft is preserved (not clobbered); the banner is what surfaces the conflict.
    expect(editor().value).toBe("my unsaved edit\n");
    expect(screen.getByTestId("docs-banner-modified")).toBeTruthy();
  });

  it("a same-doc external change HYDRATES a clean editor draft", async () => {
    seedDoc(makeView({ name: "notes", mdContent: "old\n" }));
    orchdGetDocMock.mockResolvedValue(makeView({ name: "notes", mdContent: "old\n" }));

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    const editor = () => screen.getByTestId("docs-content") as HTMLTextAreaElement;
    await waitFor(() => expect(editor().value).toBe("old\n"));

    // No local edit (clean) — a same-doc push re-hydrates the editor to the server's new content.
    const external = makeView({ name: "notes", mdContent: "server rewrite\n" });
    act(() => {
      useAppStore.setState({ docViews: { [docViewKey("p1", "notes")]: external } }, false);
    });

    expect(editor().value).toBe("server rewrite\n");
  });

  it("switching to a different doc always hydrates (navigation, not clobber)", async () => {
    useAppStore.setState(
      {
        docsByProject: { p1: [makeMeta("notes"), makeMeta("spec")] },
        docViews: {
          [docViewKey("p1", "notes")]: makeView({ name: "notes", mdContent: "notes body\n" }),
          [docViewKey("p1", "spec")]: makeView({ name: "spec", mdContent: "spec body\n" }),
        },
      },
      false,
    );
    // The mount refresh re-lists the docs — return both rows so `docs-row-spec` survives it.
    orchdListDocsMock.mockResolvedValue([makeMeta("notes"), makeMeta("spec")]);
    orchdGetDocMock.mockImplementation((_p: unknown, name: unknown) =>
      Promise.resolve(makeView({ name: name as string, mdContent: name === "spec" ? "spec body\n" : "notes body\n" })),
    );

    render(<DocsPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("docs-row-notes"));
    const editor = () => screen.getByTestId("docs-content") as HTMLTextAreaElement;
    await waitFor(() => expect(editor().value).toBe("notes body\n"));

    // Dirty the notes editor, then open a different doc — the identity change hydrates spec fully.
    fireEvent.change(editor(), { target: { value: "dirtied\n" } });
    fireEvent.click(screen.getByTestId("docs-row-spec"));
    await waitFor(() => expect(editor().value).toBe("spec body\n"));
  });
});

describe("validateDocNameDraft", () => {
  it("accepts the daemon's character class and treats empty as no-error (button-gated)", () => {
    expect(validateDocNameDraft("")).toBeNull();
    expect(validateDocNameDraft("notes")).toBeNull();
    expect(validateDocNameDraft("api-spec_v2.notes")).toBeNull();
  });

  it("rejects traversal, separators, uppercase, leading dots and over-long names", () => {
    for (const bad of ["../escape", "a/b", "a\\b", "UPPER", ".hidden", "..", "with space"]) {
      expect(validateDocNameDraft(bad)).toBe(strings.docs.invalidName);
    }
    expect(validateDocNameDraft("a".repeat(65))).toBe(strings.docs.invalidName);
  });
});

describe("formatRelativeTime", () => {
  const now = 1_700_000_000_000;

  it("buckets by age: just now, minutes, hours, days", () => {
    expect(formatRelativeTime(now - 30_000, now)).toBe(strings.docs.justNow);
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe(strings.docs.minutesAgo(5));
    expect(formatRelativeTime(now - 3 * 3_600_000, now)).toBe(strings.docs.hoursAgo(3));
    expect(formatRelativeTime(now - 9 * 86_400_000, now)).toBe(strings.docs.daysAgo(9));
  });

  it("clamps a future mtime (clock skew) to just now, never a negative age", () => {
    expect(formatRelativeTime(now + 60_000, now)).toBe(strings.docs.justNow);
  });
});
