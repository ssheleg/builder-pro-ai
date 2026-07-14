import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { orchdCreateIdea, describeOrchdError } from "../ipc/orchd";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Locked toast copy (task-19 brief verbatim) shown after a successful capture. */
const SAVED_TOAST = "идея сохранена";

/** Inline note shown instead of attempting a doomed round-trip while orchd is down (spec §10/§11
 * honest-degradation contract — mirrors every other domain surface's "never a silent no-op, never
 * a doomed send" rule). */
const ORCHD_DOWN_NOTE = "оркестратор недоступен";

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
  background: "rgba(1, 4, 9, 0.6)",
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "center",
  paddingTop: "12vh",
  zIndex: 1000,
};

const cardStyle: CSSProperties = {
  width: 480,
  background: theme.colors.bgElevated,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 10,
  boxShadow: theme.shadow,
  padding: 16,
  display: "flex",
  flexDirection: "column",
  gap: 10,
};

const titleHeadingStyle: CSSProperties = {
  fontSize: 15,
  fontWeight: 600,
  color: theme.colors.text,
};

const inputStyle: CSSProperties = {
  fontFamily: "inherit",
  fontSize: 14,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "8px 10px",
};

const textareaStyle: CSSProperties = {
  ...inputStyle,
  fontSize: 13,
  resize: "vertical",
  minHeight: 72,
};

const selectStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "6px 8px",
  alignSelf: "flex-start",
};

/** Non-amber note (design-system.md "File-state banner" atom's `accent` convention — amber stays
 * reserved for "нужен ты"): honest, not urgent — the owner just can't save right now. */
const noteStyle: CSSProperties = {
  fontSize: 13,
  lineHeight: 1.5,
  color: theme.colors.textDim,
  borderLeft: `3px solid ${theme.colors.accent}`,
  paddingLeft: 8,
};

const secondaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  fontSize: 13,
  cursor: "pointer",
};

const primaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: "none",
  background: theme.colors.accent,
  color: theme.colors.text,
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
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
 * Submit (`Enter` in the title field, or the Сохранить button) posts `orchdCreateIdea` with the
 * selected project id, or `null` for «без проекта» (the `<select>`'s empty-string sentinel maps to
 * `null` right at the call site — never sent as `""`). While `orchdDown`, the primary button is
 * disabled and an inline honest note replaces the round-trip entirely (spec §11: never a doomed
 * send). Any other rejection is surfaced via the shared toast (`describeOrchdError`), same as every
 * other domain surface — the dialog stays open so the owner can retry.
 */
export function QuickCapture(): JSX.Element | null {
  const projects = useAppStore((s) => s.projects);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);

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

  if (!open) return null;

  const blocked = orchdDown || title.trim() === "";

  return (
    <div style={overlayStyle} data-testid="quick-capture-overlay">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="quick-capture-title"
        style={cardStyle}
      >
        <div id="quick-capture-title" style={titleHeadingStyle}>
          Новая идея
        </div>

        <input
          ref={titleRef}
          data-testid="quick-capture-title-input"
          aria-label="Название идеи"
          placeholder="название"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => {
            // Enter submits from the (single-line) title field — mirrors GoalTree/IdeasList's
            // title-input Enter convention. The body textarea below gets NO such binding: Enter
            // there must insert a newline, never submit.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void handleSubmit();
            }
          }}
          style={inputStyle}
        />

        <textarea
          data-testid="quick-capture-body-input"
          aria-label="Описание идеи"
          placeholder="описание (необязательно)"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={3}
          style={textareaStyle}
        />

        <select
          data-testid="quick-capture-project-select"
          aria-label="Проект"
          value={projectId}
          onChange={(e) => setProjectId(e.target.value)}
          style={selectStyle}
        >
          <option value="">без проекта</option>
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

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            data-testid="quick-capture-cancel"
            onClick={close}
            style={secondaryButtonStyle}
          >
            Отмена
          </button>
          <button
            type="button"
            data-testid="quick-capture-submit"
            disabled={blocked}
            onClick={() => void handleSubmit()}
            style={{ ...primaryButtonStyle, opacity: blocked ? 0.5 : 1 }}
          >
            Сохранить
          </button>
        </div>
      </div>
    </div>
  );
}
