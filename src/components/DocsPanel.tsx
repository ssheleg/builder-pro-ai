import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore, docViewKey } from "../store/store";
import {
  orchdUpsertDoc,
  orchdDeleteDoc,
  orchdAcknowledgeDocFile,
  orchdRevealDocFile,
  describeOrchdError,
} from "../ipc/orchd";
import type { DocMeta } from "../ipc/orchd-types";
import { parseMarkdown, renderInline, type MdBlock } from "./markdown";
import { Button, SegmentedPill } from "../ui/primitives";
import { strings } from "../strings";

/**
 * Client-side MIRROR of the daemon's `validate_doc_name` (SCN-054: the name becomes an on-disk
 * filename component, `[a-z0-9._-]`, no leading `.`, ≤64 chars) — blocks a doomed "+ doc"
 * round-trip before it happens, same discipline as `RulesetPanel`'s `validatePolicy`. The daemon
 * stays the authoritative validator; a server-side `Validation` rejection (e.g. a race with a
 * newer rule) still surfaces via toast. An EMPTY name is not an error here — SCN-054 says empty
 * blocks the button itself (`+ doc` disabled), so only a non-empty-but-invalid draft yields the
 * inline message.
 */
export function validateDocNameDraft(name: string): string | null {
  if (name === "") return null;
  if (name.length > 64 || name.startsWith(".") || !/^[a-z0-9._-]+$/.test(name)) {
    return strings.docs.invalidName;
  }
  return null;
}

/**
 * Relative "last-modified" stamp for the doc list (SCN-054: "the list shows name +
 * last-modified"). Pure over both instants so tests pin it exactly: <60s ⇒ "just now", then
 * minutes, hours, unbounded days. A `then` in the future (clock skew between the file's mtime
 * and this machine) clamps to "just now" — never a fabricated negative age.
 */
export function formatRelativeTime(thenMs: number, nowMs: number): string {
  const elapsed = Math.max(0, nowMs - thenMs);
  if (elapsed < 60_000) return strings.docs.justNow;
  if (elapsed < 3_600_000) return strings.docs.minutesAgo(Math.floor(elapsed / 60_000));
  if (elapsed < 86_400_000) return strings.docs.hoursAgo(Math.floor(elapsed / 3_600_000));
  return strings.docs.daysAgo(Math.floor(elapsed / 86_400_000));
}

const panelStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-4)",
  alignItems: "stretch",
  fontFamily: "var(--font-ui)",
  minHeight: 0,
};

const listColumnStyle: CSSProperties = {
  width: 230,
  flexShrink: 0,
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
};

const addRowStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-2)",
  alignItems: "center",
};

const nameInputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "3px 6px",
};

const rowButtonStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-start",
  gap: 2,
  width: "100%",
  textAlign: "left",
  border: "none",
  background: "transparent",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2) var(--sp-3)",
  cursor: "pointer",
};

const editorColumnStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
};

const editorBarStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-3)",
  flexWrap: "wrap",
};

/** The rules panel's file-state banner, verbatim (SCN-054 reuses the SCN-036 pattern; see
 * `RulesetPanel.tsx`'s `bannerStyle` for the inset-shadow tone-edge rationale). */
const bannerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-3)",
  padding: "var(--sp-2) var(--sp-3)",
  boxShadow: "inset 3px 0 0 var(--info)",
  background: "var(--info-weak)",
  color: "var(--ink)",
  fontSize: "var(--fs-md)",
  borderRadius: "var(--r-sm)",
};

const textareaStyle: CSSProperties = {
  width: "100%",
  minHeight: 260,
  boxSizing: "border-box",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2) var(--sp-3)",
  resize: "vertical",
};

const previewStyle: CSSProperties = {
  minHeight: 260,
  background: "var(--panel-2)",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-3) var(--sp-4)",
  color: "var(--ink)",
  fontSize: "var(--fs-md)",
  overflowY: "auto",
};

const errorTextStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--danger)",
};

const mutedTextStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-md)",
};

/** Heading font sizes by level for the preview — h1 largest, h4+ settle at body weight. */
const HEADING_SIZES = [
  "var(--fs-xl)",
  "var(--fs-lg)",
  "var(--fs-md)",
  "var(--fs-md)",
  "var(--fs-md)",
  "var(--fs-md)",
] as const;

/** Rendered-markdown preview (SCN-054 step 3): maps `parseMarkdown`'s blocks to plain JSX —
 * never HTML strings, so doc content cannot inject markup. */
function MarkdownPreview(props: { markdown: string }): JSX.Element {
  const blocks = parseMarkdown(props.markdown);
  return (
    <div data-testid="docs-preview" style={previewStyle}>
      {blocks.map((block, i) => renderBlock(block, i))}
    </div>
  );
}

function renderBlock(block: MdBlock, key: number): JSX.Element {
  switch (block.kind) {
    case "heading":
      return (
        <div
          key={key}
          role="heading"
          aria-level={block.level}
          style={{
            fontSize: HEADING_SIZES[block.level - 1],
            fontWeight: 700,
            margin: "var(--sp-2) 0",
          }}
        >
          {renderInline(block.text)}
        </div>
      );
    case "paragraph":
      return (
        <p key={key} style={{ margin: "var(--sp-2) 0" }}>
          {renderInline(block.text)}
        </p>
      );
    case "list": {
      const items = block.items.map((item, j) => <li key={j}>{renderInline(item)}</li>);
      return block.ordered ? (
        <ol key={key} style={{ margin: "var(--sp-2) 0", paddingLeft: "var(--sp-5)" }}>
          {items}
        </ol>
      ) : (
        <ul key={key} style={{ margin: "var(--sp-2) 0", paddingLeft: "var(--sp-5)" }}>
          {items}
        </ul>
      );
    }
    case "code":
      return (
        <pre
          key={key}
          style={{
            margin: "var(--sp-2) 0",
            padding: "var(--sp-2) var(--sp-3)",
            background: "var(--panel)",
            borderRadius: "var(--r-sm)",
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-sm)",
            overflowX: "auto",
          }}
        >
          {block.text}
        </pre>
      );
  }
}

/**
 * Project documentation tab (SCN-054, FLW-21, ST-041): per-project markdown documents,
 * file-backed alongside rules.md ("rules.md × N named files") so agents read the same files from
 * the project directory. Left: the doc list (name + relative last-modified; "+ doc" with a name
 * input — empty name disables the button, an invalid one shows the inline mirror of the daemon's
 * name rule). Right: the editor for the selected doc — an edit/preview `SegmentedPill`
 * (preview renders the current DRAFT through the dependency-free `parseMarkdown`), "reveal file"
 * (CORE-ONLY path resolution, spec §9 — never a path from JS), Delete behind the locked
 * "delete document?" confirm, and the two SCN-036-pattern file-state banners: `externallyModified`
 * ⇒ "file changed externally" + [Accept] (`AcknowledgeDocFile`), `missing` ⇒ "file lost" +
 * [Recreate] (`UpsertDoc{mdContent: ""}` — the exact rules recreate path). `mdContent` is only
 * ever a DRAFT of the on-disk file; the store's copy wins whenever a fresh `GetDoc` lands (the
 * same "store wins over a stale local draft" discipline as `RulesetPanel`).
 *
 * Save failure (SCN-054 errors): the editor keeps the draft VERBATIM and surfaces the mapped
 * message BOTH inline (next to Save) and as a toast — content is never discarded on a rejection.
 *
 * Honest degradation (SCN-054): while `orchdDown`, every mutating control ("+ doc", Save,
 * Delete, Accept, Recreate) is disabled; reading the loaded list/content, the edit/preview
 * toggle, and "reveal file" stay live (`ProjectPanel` owns the shared banner; this component
 * only disables its own controls — the `RulesetPanel` split).
 *
 * Refresh discipline: the list re-fetches on mount/project change; the selected doc re-fetches
 * on selection and after every successful mutation (the `orchd://docs-changed` push binding in
 * `App.tsx` covers cross-client changes, mirroring every other domain slice).
 */
export function DocsPanel(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const docs = useAppStore((s) => s.docsByProject[projectId]);
  const refreshDocs = useAppStore((s) => s.refreshDocs);
  const refreshDoc = useAppStore((s) => s.refreshDoc);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

  const [selectedName, setSelectedName] = useState<string | null>(null);
  const view = useAppStore((s) =>
    selectedName === null ? undefined : s.docViews[docViewKey(projectId, selectedName)],
  );

  const [draftName, setDraftName] = useState("");
  const [nameError, setNameError] = useState<string | null>(null);
  const [content, setContent] = useState(view?.mdContent ?? "");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [mode, setMode] = useState<"edit" | "preview">("edit");

  // Dirty-draft guard baseline (PRN-03): the last content hydrated from the store, tagged with the
  // selected doc name. On a same-doc re-hydrate (an `orchd://docs-changed` push or reconnect
  // rehydrate landing mid-edit) the editor keeps a draft the user has edited away from this
  // baseline (dirty) and only re-adopts a still-matching one (clean) — the "file changed
  // externally" banner mediates the conflict, so the guard just stops the silent pre-banner
  // clobber. Selecting a different doc (identity change) always hydrates fully. `null` until first
  // hydrate.
  const hydratedDocRef = useRef<{ name: string | null; content: string } | null>(null);

  // The list re-fetches on mount/project change (SCN-054 step 1); switching projects must never
  // keep the previous project's selection/drafts (the ProjectPanel import-state discipline).
  useEffect(() => {
    void refreshDocs(projectId);
    setSelectedName(null);
    setDraftName("");
    setNameError(null);
    setSaveError(null);
    setMode("edit");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Selecting a doc re-fetches its view fresh (spec §7's "read file fresh each time", applied to
  // docs — the file can change on disk with no push to invalidate it).
  useEffect(() => {
    if (selectedName !== null) void refreshDoc(projectId, selectedName);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, selectedName]);

  // Store wins over a stale local draft whenever a fresh view lands (mount refresh, a mutation's
  // own post-success refresh, or the `orchd://docs-changed` push binding in App.tsx) — WITH the
  // dirty-draft guard (PRN-03, see `hydratedDocRef` above): a same-doc update keeps an in-progress
  // edit (the banner mediates), a clean editor re-hydrates, and selecting a different doc always
  // hydrates fully.
  useEffect(() => {
    const nextContent = view?.mdContent ?? "";
    const baseline = hydratedDocRef.current;
    const sameDoc = baseline !== null && baseline.name === selectedName;
    if (!sameDoc) {
      setContent(nextContent);
      setSaveError(null);
    } else {
      setContent((cur) => (cur === baseline.content ? nextContent : cur));
    }
    // Always advance the baseline to the latest server content so the next clean hydrate works.
    hydratedDocRef.current = { name: selectedName, content: nextContent };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view?.mdContent, selectedName]);

  const trimmedDraft = draftName.trim();

  async function handleCreate(): Promise<void> {
    const error = validateDocNameDraft(trimmedDraft);
    if (error !== null) {
      setNameError(error);
      return;
    }
    // Duplicate guard (SCN-054): `UpsertDoc` is upsert-shaped on the wire (the rules-template
    // minimal verb set), so "+ doc" over an existing name would blank that doc's file with the
    // empty create content — blocked here against the push-fresh list instead.
    if ((docs ?? []).some((d) => d.name === trimmedDraft)) {
      setNameError(strings.docs.duplicateName);
      return;
    }
    setNameError(null);
    try {
      await orchdUpsertDoc(projectId, trimmedDraft, "");
      setDraftName("");
      setSelectedName(trimmedDraft);
      await refreshDocs(projectId);
      await refreshDoc(projectId, trimmedDraft);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleSave(): Promise<void> {
    if (selectedName === null) return;
    try {
      await orchdUpsertDoc(projectId, selectedName, content);
      setSaveError(null);
      await refreshDoc(projectId, selectedName);
      // The list's last-modified stamp moved with the file write.
      await refreshDocs(projectId);
    } catch (e) {
      // SCN-054: inline + toast, editor content preserved — the draft state is deliberately NOT
      // touched here.
      const message = describeOrchdError(e);
      setSaveError(message);
      showToast(message);
    }
  }

  async function handleAcknowledge(): Promise<void> {
    if (view === undefined || selectedName === null) return;
    try {
      await orchdAcknowledgeDocFile(view.doc.id);
      await refreshDoc(projectId, selectedName);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleRecreate(): Promise<void> {
    if (selectedName === null) return;
    try {
      await orchdUpsertDoc(projectId, selectedName, "");
      await refreshDoc(projectId, selectedName);
      await refreshDocs(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleDelete(): Promise<void> {
    if (view === undefined || selectedName === null) return;
    if (!window.confirm(strings.docs.deleteConfirm)) return;
    const name = selectedName;
    try {
      await orchdDeleteDoc(view.doc.id);
      setSelectedName(null);
      await refreshDocs(projectId);
      // Drops the now-dangling view entry via refreshDoc's NotFound handling (store hygiene).
      await refreshDoc(projectId, name);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleReveal(): Promise<void> {
    if (selectedName === null) return;
    try {
      await orchdRevealDocFile(projectId, selectedName);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  const now = Date.now();

  function renderList(): JSX.Element {
    if (docs === undefined) {
      return (
        <div data-testid="docs-loading" style={mutedTextStyle}>
          {strings.docs.loading}
        </div>
      );
    }
    if (docs.length === 0) {
      return (
        <div data-testid="docs-empty" style={mutedTextStyle}>
          {strings.docs.empty}
        </div>
      );
    }
    return (
      <ul
        data-testid="docs-list"
        aria-label={strings.docs.listAria}
        style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 2 }}
      >
        {docs.map((d: DocMeta) => {
          const active = d.name === selectedName;
          return (
            <li key={d.name}>
              <button
                type="button"
                data-testid={`docs-row-${d.name}`}
                aria-label={strings.docs.docRowAria(d.name)}
                aria-current={active}
                onClick={() => setSelectedName(d.name)}
                style={{
                  ...rowButtonStyle,
                  background: active ? "var(--accent-weak)" : "transparent",
                }}
              >
                <span style={{ fontSize: "var(--fs-md)", fontWeight: active ? 600 : 400, color: "var(--ink)", wordBreak: "break-all" }}>
                  {d.name}
                </span>
                <span style={{ fontSize: "var(--fs-xs)", color: "var(--muted)" }}>
                  {formatRelativeTime(d.modifiedAt, now)}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    );
  }

  function renderEditor(): JSX.Element {
    if (selectedName === null) {
      return (
        <div data-testid="docs-select-prompt" style={mutedTextStyle}>
          {strings.docs.selectPrompt}
        </div>
      );
    }
    if (view === undefined) {
      return (
        <div data-testid="docs-doc-loading" style={mutedTextStyle}>
          {strings.docs.loadingDoc}
        </div>
      );
    }
    return (
      <>
        <div style={editorBarStyle}>
          <span style={{ fontSize: "var(--fs-md)", fontWeight: 600, color: "var(--ink)", wordBreak: "break-all" }}>
            {view.doc.name}
          </span>
          <SegmentedPill
            options={[
              { value: "edit", label: strings.docs.modeEdit },
              { value: "preview", label: strings.docs.modePreview },
            ]}
            value={mode}
            onChange={setMode}
            ariaLabel={strings.docs.modeAria}
            data-testid="docs-mode"
          />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-testid="docs-reveal"
            onClick={() => void handleReveal()}
            style={{ flexShrink: 0, whiteSpace: "nowrap" }}
          >
            {strings.docs.revealFile}
          </Button>
          <Button
            type="button"
            variant="danger"
            size="sm"
            data-testid="docs-delete"
            disabled={orchdDown}
            onClick={() => void handleDelete()}
            style={{ flexShrink: 0, whiteSpace: "nowrap", marginLeft: "auto" }}
          >
            {strings.common.delete}
          </Button>
        </div>

        {view.fileState === "externallyModified" && (
          <div data-testid="docs-banner-modified" role="status" style={bannerStyle}>
            <span style={{ flex: 1 }}>{strings.docs.modifiedBanner}</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-testid="docs-acknowledge"
              disabled={orchdDown}
              onClick={() => void handleAcknowledge()}
              style={{ flexShrink: 0, whiteSpace: "nowrap" }}
            >
              {strings.common.accept}
            </Button>
          </div>
        )}

        {view.fileState === "missing" && (
          <div data-testid="docs-banner-missing" role="status" style={bannerStyle}>
            <span style={{ flex: 1 }}>{strings.docs.missingBanner}</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-testid="docs-recreate"
              disabled={orchdDown}
              onClick={() => void handleRecreate()}
              style={{ flexShrink: 0, whiteSpace: "nowrap" }}
            >
              {strings.docs.recreate}
            </Button>
          </div>
        )}

        {view.fileState !== "missing" && (
          <>
            {mode === "edit" ? (
              <textarea
                data-testid="docs-content"
                aria-label={strings.docs.editorAria}
                value={content}
                onChange={(e) => {
                  setContent(e.target.value);
                  setSaveError(null);
                }}
                rows={14}
                style={textareaStyle}
              />
            ) : (
              <MarkdownPreview markdown={content} />
            )}
            {saveError !== null && (
              <span data-testid="docs-save-error" style={errorTextStyle}>
                {saveError}
              </span>
            )}
            <Button
              type="button"
              variant="primary"
              size="sm"
              data-testid="docs-save"
              disabled={orchdDown}
              onClick={() => void handleSave()}
              style={{ alignSelf: "flex-start" }}
            >
              {strings.common.save}
            </Button>
          </>
        )}
      </>
    );
  }

  return (
    <div data-testid="docs-panel" style={panelStyle}>
      <div style={listColumnStyle}>
        <div style={addRowStyle}>
          <input
            data-testid="docs-add-name"
            aria-label={strings.docs.nameAria}
            placeholder={strings.docs.namePlaceholder}
            value={draftName}
            onChange={(e) => {
              setDraftName(e.target.value);
              setNameError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && trimmedDraft !== "" && !orchdDown) {
                e.preventDefault();
                void handleCreate();
              }
            }}
            style={nameInputStyle}
          />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-testid="docs-add"
            // SCN-054: empty name → "+ doc" blocked; orchd down → mutation disabled.
            disabled={trimmedDraft === "" || orchdDown}
            onClick={() => void handleCreate()}
            style={{ flexShrink: 0, whiteSpace: "nowrap" }}
          >
            {strings.docs.addDoc}
          </Button>
        </div>
        {nameError !== null && (
          <span data-testid="docs-name-error" style={errorTextStyle}>
            {nameError}
          </span>
        )}
        {renderList()}
      </div>
      <div style={editorColumnStyle}>{renderEditor()}</div>
    </div>
  );
}
