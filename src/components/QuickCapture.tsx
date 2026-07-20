import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { orchdCreateIdea, describeOrchdError } from "../ipc/orchd";
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { Button } from "../ui/primitives";
import { strings } from "../strings";

/** Locked toast copy (task-19 brief verbatim) shown after a successful capture. */
const SAVED_TOAST = strings.capture.ideaSaved;

/** Inline note shown instead of attempting a doomed round-trip while orchd is down (spec §10/§11
 * honest-degradation contract — mirrors every other domain surface's "never a silent no-op, never
 * a doomed send" rule). */
const ORCHD_DOWN_NOTE = strings.errors.unavailable;

/**
 * `true` when the given element is a place the owner is already typing — a plain text input/
 * textarea, OR anywhere inside an xterm terminal pane (xterm keeps its own hidden
 * `.xterm-helper-textarea` for keystroke capture, which the tag check alone already catches, but
 * the `.xterm` ancestor check is belt-and-suspenders per the task-19 brief: "check activeElement's
 * tagName / a `.xterm` ancestor"). `⌘K` must never steal a `k` keystroke the owner is typing
 * somewhere else in the app.
 */
function isTypingTarget(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return true;
  return el.closest(".xterm") !== null;
}

const overlayStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(0, 0, 0, 0.4)",
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "center",
  paddingTop: "12vh",
  zIndex: 1000,
};

const cardStyle: CSSProperties = {
  width: 480,
  background: "var(--panel)",
  borderRadius: "var(--r-lg)",
  boxShadow: "var(--shadow-1)",
  padding: "var(--sp-4)",
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
};

const titleHeadingStyle: CSSProperties = {
  fontSize: "var(--fs-lg)",
  fontWeight: 600,
  color: "var(--ink)",
};

const inputStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-md)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2) var(--sp-3)",
};

const textareaStyle: CSSProperties = {
  ...inputStyle,
  fontSize: "var(--fs-sm)",
  resize: "vertical",
  minHeight: 72,
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2) var(--sp-2)",
  alignSelf: "flex-start",
};

/** Non-amber note (design-system.md "File-state banner" atom's `accent` convention — amber stays
 * reserved for "needs you"): honest, not urgent — the owner just can't save right now. */
const noteStyle: CSSProperties = {
  fontSize: "var(--fs-md)",
  lineHeight: 1.5,
  color: "var(--muted)",
  borderLeft: "3px solid var(--accent)",
  paddingLeft: "var(--sp-2)",
};

/**
 * Global ⌘K idea-capture overlay (spec §10, task-19). Self-gated on its OWN internal `open`
 * state (dialog-atom parity with `UpgradeDialog`/`CreateProjectDialog`) rather than a store flag —
 * there is nothing else in the app that needs to know "quick capture is open", so this is pure
 * local UI state. Mounted exactly ONCE, app-wide, in `App.tsx` (its `⌘K` listener must be live
 * regardless of which view is showing).
 *
 * The `⌘K` listener is unconditional (registered once, empty deps) — it always re-reads
 * `document.activeElement` at keydown time via `isTypingTarget`, so it never steals the
 * keystroke from an input/textarea/terminal the owner is actively typing into (task-19 brief).
 * A second, `open`-scoped effect owns focus-on-open + `Escape`-to-close, mirroring
 * `CreateProjectDialog`'s identical pattern.
 *
 * Submit (`Enter` in the title field, or the Save button) posts `orchdCreateIdea` with the
 * selected project id, or `null` for "no project" (the `<select>`'s empty-string sentinel maps to
 * `null` right at the call site — never sent as `""`). While `orchdDown`, the primary button is
 * disabled and an inline honest note replaces the round-trip entirely (spec §11: never a doomed
 * send). Any other rejection is surfaced via the shared toast (`describeOrchdError`), same as every
 * other domain surface — the dialog stays open so the owner can retry.
 */
export function QuickCapture(): JSX.Element | null {
  const projects = useAppStore((s) => s.projects);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);
  const { submitting, guard } = useSubmitGuard();

  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [projectId, setProjectId] = useState("");

  const titleRef = useRef<HTMLInputElement>(null);

  function close(): void {
    setOpen(false);
    setTitle("");
    setBody("");
    setProjectId("");
  }

  // Unconditional global ⌘K listener (task-19 brief: "Register the global keydown listener in
  // QuickCapture's own effect"). Never depends on `open` — it must keep listening even while the
  // overlay itself is closed, which is the whole point of a global shortcut. Empty deps, so it
  // never re-reads a stale closure: `isTypingTarget(document.activeElement)` re-reads the DOM
  // live at keydown time, and `useAppStore.getState()` (rather than a subscribed hook value)
  // re-reads the store live at keydown time for the same reason.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent): void {
      if (!e.metaKey || e.key.toLowerCase() !== "k") return;
      if (isTypingTarget(document.activeElement)) return;
      // Also never steal ⌘K out from under a MANDATORY upgrade dialog (UpgradeDialog focuses a
      // button, not an input/textarea, so `isTypingTarget` alone doesn't catch this case) — a
      // blocking daemon/orchd version-incompatibility dialog must not have quick-capture opened
      // on top of it.
      const s = useAppStore.getState();
      if (s.daemonIncompatible && s.upgradeDialogOpen) return;
      if (s.orchdIncompatible && s.orchdUpgradeDialogOpen) return;
      e.preventDefault();
      setOpen(true);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // Focus-on-open + Escape-to-close (dialog-atom parity, mirrors CreateProjectDialog/UpgradeDialog).
  useEffect(() => {
    if (!open) return;
    titleRef.current?.focus();
    function onKeyDown(e: KeyboardEvent): void {
      if (e.key === "Escape") close();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  async function handleSubmit(): Promise<void> {
    const trimmed = title.trim();
    if (trimmed === "" || orchdDown) return; // honest guard — never a doomed send (spec §11)
    try {
      await orchdCreateIdea(projectId === "" ? null : projectId, trimmed, body);
      showToast(SAVED_TOAST);
      close();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second Enter/click before the first `orchdCreateIdea`
  // resolves must NOT create a second idea (finding E-08).
  const submit = guard(handleSubmit);

  if (!open) return null;

  const blocked = orchdDown || title.trim() === "" || submitting;

  return (
    <div style={overlayStyle} data-testid="quick-capture-overlay">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="quick-capture-title"
        style={cardStyle}
      >
        <div id="quick-capture-title" style={titleHeadingStyle}>
          {strings.capture.newIdea}
        </div>

        <input
          ref={titleRef}
          data-testid="quick-capture-title-input"
          aria-label={strings.capture.titleAria}
          placeholder={strings.capture.titlePlaceholder}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => {
            // Enter submits from the (single-line) title field — mirrors GoalTree/IdeasList's
            // title-input Enter convention. The body textarea below gets NO such binding: Enter
            // there must insert a newline, never submit.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit();
            }
          }}
          style={inputStyle}
        />

        <textarea
          data-testid="quick-capture-body-input"
          aria-label={strings.capture.descriptionAria}
          placeholder={strings.common.descriptionOptional}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={3}
          style={textareaStyle}
        />

        <select
          data-testid="quick-capture-project-select"
          aria-label={strings.capture.projectAria}
          value={projectId}
          onChange={(e) => setProjectId(e.target.value)}
          style={selectStyle}
        >
          <option value="">{strings.capture.noProject}</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>

        {orchdDown && (
          <div data-testid="quick-capture-orchd-down" role="status" style={noteStyle}>
            {ORCHD_DOWN_NOTE}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: "var(--sp-2)", marginTop: "var(--sp-1)" }}>
          <Button
            variant="ghost"
            type="button"
            data-testid="quick-capture-cancel"
            onClick={close}
          >
            {strings.common.cancel}
          </Button>
          <Button
            variant="primary"
            type="button"
            data-testid="quick-capture-submit"
            disabled={blocked}
            onClick={() => void submit()}
          >
            {strings.common.save}
          </Button>
        </div>
      </div>
    </div>
  );
}
