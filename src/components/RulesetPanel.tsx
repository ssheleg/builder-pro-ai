import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdUpsertRuleset,
  orchdAcknowledgeRuleFile,
  orchdRevealRulesFile,
  describeOrchdError,
} from "../ipc/orchd";
import type { PolicyRules, RuleScope } from "../ipc/orchd-types";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Locked banner copy for the `missing` file state (task-17 brief verbatim: «файл утерян»). No
 * copy is locked for `externallyModified` — this one is written to the same terse honesty
 * register. */
const MISSING_BANNER_TEXT = "файл утерян";
const MODIFIED_BANNER_TEXT = "файл изменён снаружи";

/**
 * Store-key format shared with `store.ts`'s `rulesets`/`parseRulesetKey` (spec §10): the global
 * scope's key is the literal `"global"`, every project scope is `` `project:${id}` ``.
 */
function rulesetKey(scope: RuleScope, projectId: string | null): string {
  return scope === "global" ? "global" : `project:${projectId}`;
}

/**
 * Client-side MIRROR of the server's `PolicyRules` strict validation (spec §5.2:
 * `deny_unknown_fields`, `spend_cap_usd >= 0`, non-empty allowlist/approval-class entries) —
 * blocks a doomed request before it round-trips, same discipline as `InsightsList`'s inline
 * archive-reasoning guard. `approvalClasses`/`pathAllowlist` can never actually contain an empty
 * entry here (the `ChipList` adder trims and refuses blanks), but this still re-checks them
 * defensively so the guard holds even if that invariant is ever broken upstream. A parseable but
 * negative cap is blocked; a non-numeric cap is blocked too (never silently coerced to 0/NaN).
 */
function validatePolicy(
  spendCapText: string,
  approvalClasses: string[],
  pathAllowlist: string[],
): { policy: PolicyRules } | { error: string } {
  const trimmedCap = spendCapText.trim();
  let spendCapUsd: number | null = null;
  if (trimmedCap !== "") {
    const parsed = Number(trimmedCap);
    if (!Number.isFinite(parsed)) {
      return { error: "лимит расходов должен быть числом" };
    }
    if (parsed < 0) {
      return { error: "лимит расходов не может быть отрицательным" };
    }
    spendCapUsd = parsed;
  }
  if (approvalClasses.some((c) => c.trim() === "") || pathAllowlist.some((p) => p.trim() === "")) {
    return { error: "пустые записи недопустимы" };
  }
  return { policy: { spendCapUsd, approvalClasses, pathAllowlist } };
}

const panelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 12,
  fontFamily: MONO_FONT,
};

const pathRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  flexWrap: "wrap",
};

const pathTextStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.textDim,
  wordBreak: "break-all",
};

/** Info banner (design-system.md "File-state banner" atom, task-17): the SAME inbox-item shape as
 * `DaemonBanner`'s incompatible case (left-edge accent + text + inline action) but with `accent`
 * (the one neutral/info color) instead of `statusWaiting` — amber is reserved for "нужен ты"
 * (design-system.md §2), and a stale/missing rules file is informational, not a human-attention
 * gate. */
const bannerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 12,
  padding: "6px 12px",
  borderLeft: `3px solid ${theme.colors.accent}`,
  background: theme.colors.bgElevated,
  color: theme.colors.text,
  fontSize: 13,
  borderRadius: 4,
};

const textareaStyle: CSSProperties = {
  width: "100%",
  minHeight: 200,
  boxSizing: "border-box",
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "8px 10px",
  resize: "vertical",
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 11,
  borderRadius: 4,
  padding: "2px 6px",
  flexShrink: 0,
  whiteSpace: "nowrap",
};

const primaryButtonStyle: CSSProperties = {
  ...textButtonStyle,
  color: theme.colors.bg,
  background: theme.colors.accent,
  borderColor: theme.colors.accent,
  alignSelf: "flex-start",
};

const policyFormStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
  padding: "8px 12px",
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
  background: theme.colors.bgElevated,
};

const labelStyle: CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: theme.colors.textDim,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const numberInputStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "3px 6px",
  maxWidth: 140,
};

const errorTextStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.statusExited,
};

const chipRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 6,
};

const chipStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  fontFamily: MONO_FONT,
  fontSize: 11,
  padding: "2px 8px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.border}`,
  color: theme.colors.text,
};

const chipRemoveStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: theme.colors.textDim,
  cursor: "pointer",
  fontSize: 12,
  lineHeight: 1,
  padding: 0,
};

const chipInputStyle: CSSProperties = {
  flex: "1 1 140px",
  minWidth: 100,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "3px 6px",
};

interface ChipListProps {
  testIdPrefix: string;
  ariaLabel: string;
  placeholder: string;
  values: string[];
  onAdd: (v: string) => void;
  onRemove: (v: string) => void;
}

/** Chip/list input for `approvalClasses`/`pathAllowlist` (design-system.md "Policy form" atom,
 * task-17): existing entries render as Chip atoms with a `×` remove; a trimmed, non-empty draft is
 * added via the button or Enter — an empty/whitespace-only draft is a silent no-op, so this widget
 * can never itself produce the "empty entry" validation error `validatePolicy` guards against. */
function ChipList(props: ChipListProps): JSX.Element {
  const { testIdPrefix, ariaLabel, placeholder, values, onAdd, onRemove } = props;
  const [draft, setDraft] = useState("");

  function commitAdd(): void {
    const trimmed = draft.trim();
    if (trimmed === "") return;
    onAdd(trimmed);
    setDraft("");
  }

  return (
    <div style={chipRowStyle}>
      {values.map((v) => (
        <span key={v} data-testid={`${testIdPrefix}-chip-${v}`} style={chipStyle}>
          {v}
          <button
            type="button"
            data-testid={`${testIdPrefix}-remove-${v}`}
            aria-label={`Удалить ${v}`}
            onClick={() => onRemove(v)}
            style={chipRemoveStyle}
          >
            ×
          </button>
        </span>
      ))}
      <input
        data-testid={`${testIdPrefix}-input`}
        aria-label={ariaLabel}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commitAdd();
          }
        }}
        style={chipInputStyle}
      />
      <button
        type="button"
        data-testid={`${testIdPrefix}-add`}
        onClick={commitAdd}
        style={textButtonStyle}
      >
        + добавить
      </button>
    </div>
  );
}

/**
 * Rules editor (S3 spec §7/§10, task-17). Markdown files are the source of truth — `mdContent`
 * here is only ever a DRAFT of the on-disk file, never authoritative until a Save round-trips.
 * `fileState` drives which affordances render: `ok` ⇒ a plain editable textarea, no banner;
 * `externallyModified` ⇒ the on-disk content (spec §7: "content returned") PLUS an info banner
 * offering [Принять] to accept the new hash without discarding the owner's ability to instead
 * just overwrite it with Save; `missing` ⇒ no textarea (there is no content to bind — `mdContent`
 * is `null`), only a banner offering [Создать заново] (`UpsertRuleSet{mdContent: ""}`, spec §7's
 * documented recreate path).
 *
 * The policy form is INDEPENDENT of file state — `RuleSet.policy` is a DB column, not file
 * content — so it always renders, whatever `fileState` says. Client-side validation
 * (`validatePolicy`) MIRRORS the server's strict `PolicyRules` checks (spec §5.2) and blocks a
 * negative spend cap before ever calling `orchdUpsertRuleset`; a server-side `Validation` error
 * (e.g. from a race with another client) still surfaces verbatim via `showToast(describeOrchdError
 * (e))` — the client-side guard is a UX nicety, never the only line of defense (spec §7 honest
 * error surface).
 *
 * `orchdRevealRulesFile(scope, projectId)` — never a path from JS (spec §9: "Rules-file reveal (no
 * arbitrary paths from JS)"); the core re-derives `md_path` from its own fresh `GetRuleSet`.
 *
 * Refreshes on mount (spec §10: "Refreshes on `orchd://ruleset-changed` + on mount" — the push
 * binding itself lives in App.tsx's shared event-wiring effect, same as every other domain slice;
 * this component only owns the on-mount re-Get, unconditionally — unlike `GoalTree`'s "only if
 * empty" cache check, spec §7 explicitly wants a fresh read every time the panel opens since the
 * file can change on disk with no push to invalidate it) and after every successful mutation
 * (Save/Принять/Создать заново/policy Save) so the UI reflects the new hash/state immediately
 * rather than waiting on a `RuleSetChanged` push that may not even fire for every one of these
 * (e.g. Acknowledge is a pure client action from the daemon's perspective in some races).
 */
export function RulesetPanel(props: { scope: RuleScope; projectId: string | null }): JSX.Element {
  const { scope, projectId } = props;
  const key = rulesetKey(scope, projectId);

  const view = useAppStore((s) => s.rulesets[key]);
  const refreshRuleset = useAppStore((s) => s.refreshRuleset);
  const showToast = useAppStore((s) => s.showToast);

  const [content, setContent] = useState(view?.mdContent ?? "");
  const [spendCapText, setSpendCapText] = useState(
    view?.rule.policy.spendCapUsd == null ? "" : String(view.rule.policy.spendCapUsd),
  );
  const [approvalClasses, setApprovalClasses] = useState<string[]>(
    view?.rule.policy.approvalClasses ?? [],
  );
  const [pathAllowlist, setPathAllowlist] = useState<string[]>(view?.rule.policy.pathAllowlist ?? []);
  const [policyError, setPolicyError] = useState<string | null>(null);

  // Always re-Get on mount/scope change — see the doc comment above for why this differs from
  // GoalTree's "only if empty" cache check.
  useEffect(() => {
    void refreshRuleset(key);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // The store's copy only changes once a fresh view lands (mount refresh, a mutation's own
  // post-success refresh, or a future `orchd://ruleset-changed` push) — sync local drafts to it
  // whenever that happens, same "store wins over a stale local draft" discipline as GoalTree's
  // GoalRow.
  useEffect(() => {
    setContent(view?.mdContent ?? "");
  }, [view?.mdContent]);
  useEffect(() => {
    setSpendCapText(view?.rule.policy.spendCapUsd == null ? "" : String(view.rule.policy.spendCapUsd));
    setApprovalClasses(view?.rule.policy.approvalClasses ?? []);
    setPathAllowlist(view?.rule.policy.pathAllowlist ?? []);
    setPolicyError(null);
    // `policy` is a fresh object every fetch — comparing the object reference is intentional here,
    // it is exactly "a new GetRuleSet reply landed".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view?.rule.policy]);

  if (!view) {
    return (
      <div data-testid="ruleset-panel-loading" style={{ color: theme.colors.textDim, fontSize: 13 }}>
        Загрузка правил…
      </div>
    );
  }

  const { rule, fileState } = view;

  async function handleSaveContent(): Promise<void> {
    try {
      await orchdUpsertRuleset(scope, projectId, content, null, null);
      await refreshRuleset(key);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleAcknowledge(): Promise<void> {
    try {
      await orchdAcknowledgeRuleFile(rule.id);
      await refreshRuleset(key);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleRecreate(): Promise<void> {
    try {
      await orchdUpsertRuleset(scope, projectId, "", null, null);
      await refreshRuleset(key);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleReveal(): Promise<void> {
    try {
      await orchdRevealRulesFile(scope, projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleSavePolicy(): Promise<void> {
    const result = validatePolicy(spendCapText, approvalClasses, pathAllowlist);
    if ("error" in result) {
      setPolicyError(result.error);
      return;
    }
    setPolicyError(null);
    try {
      await orchdUpsertRuleset(scope, projectId, null, null, result.policy);
      await refreshRuleset(key);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  return (
    <div data-testid="ruleset-panel" style={panelStyle}>
      <div style={pathRowStyle}>
        <span data-testid="ruleset-path" style={pathTextStyle}>
          {rule.mdPath}
        </span>
        <button
          type="button"
          data-testid="ruleset-reveal"
          onClick={() => void handleReveal()}
          style={textButtonStyle}
        >
          показать файл
        </button>
      </div>

      {fileState === "externallyModified" && (
        <div data-testid="ruleset-banner-modified" role="status" style={bannerStyle}>
          <span>{MODIFIED_BANNER_TEXT}</span>
          <button
            type="button"
            data-testid="ruleset-acknowledge"
            onClick={() => void handleAcknowledge()}
            style={textButtonStyle}
          >
            Принять
          </button>
        </div>
      )}

      {fileState === "missing" && (
        <div data-testid="ruleset-banner-missing" role="status" style={bannerStyle}>
          <span>{MISSING_BANNER_TEXT}</span>
          <button
            type="button"
            data-testid="ruleset-recreate"
            onClick={() => void handleRecreate()}
            style={textButtonStyle}
          >
            Создать заново
          </button>
        </div>
      )}

      {fileState !== "missing" && (
        <>
          <textarea
            data-testid="ruleset-content"
            aria-label="Содержимое правил"
            value={content}
            onChange={(e) => setContent(e.target.value)}
            rows={12}
            style={textareaStyle}
          />
          <button
            type="button"
            data-testid="ruleset-save-content"
            onClick={() => void handleSaveContent()}
            style={primaryButtonStyle}
          >
            Сохранить
          </button>
        </>
      )}

      <div data-testid="ruleset-policy-form" style={policyFormStyle}>
        <span style={labelStyle}>Лимит расходов, $</span>
        <input
          data-testid="ruleset-spend-cap"
          aria-label="Лимит расходов в долларах, пусто — без лимита"
          type="number"
          placeholder="без лимита"
          value={spendCapText}
          onChange={(e) => {
            setSpendCapText(e.target.value);
            setPolicyError(null);
          }}
          style={numberInputStyle}
        />

        <span style={labelStyle}>Классы, требующие подтверждения</span>
        <ChipList
          testIdPrefix="ruleset-approval-class"
          ariaLabel="Новый класс подтверждения"
          placeholder="класс"
          values={approvalClasses}
          onAdd={(v) => {
            setApprovalClasses((prev) => [...prev, v]);
            setPolicyError(null);
          }}
          onRemove={(v) => setApprovalClasses((prev) => prev.filter((c) => c !== v))}
        />

        <span style={labelStyle}>Разрешённые пути</span>
        <ChipList
          testIdPrefix="ruleset-allowlist"
          ariaLabel="Новый разрешённый путь"
          placeholder="путь"
          values={pathAllowlist}
          onAdd={(v) => {
            setPathAllowlist((prev) => [...prev, v]);
            setPolicyError(null);
          }}
          onRemove={(v) => setPathAllowlist((prev) => prev.filter((p) => p !== v))}
        />

        {policyError !== null && (
          <span data-testid="ruleset-policy-error" style={errorTextStyle}>
            {policyError}
          </span>
        )}

        <button
          type="button"
          data-testid="ruleset-save-policy"
          onClick={() => void handleSavePolicy()}
          style={primaryButtonStyle}
        >
          Сохранить политику
        </button>
      </div>
    </div>
  );
}
