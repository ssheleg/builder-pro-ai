import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdUpsertRuleset,
  orchdAcknowledgeRuleFile,
  orchdRevealRulesFile,
  describeOrchdError,
} from "../ipc/orchd";
import type { PolicyRules, RuleScope } from "../ipc/orchd-types";
import { Badge, Button } from "../ui/primitives";
import { strings } from "../strings";

/** Locked banner copy for the `missing` file state (task-17 brief verbatim: "file lost"). No
 * copy is locked for `externallyModified` — this one is written to the same terse honesty
 * register. */
const MISSING_BANNER_TEXT = strings.rules.missingBanner;
const MODIFIED_BANNER_TEXT = strings.rules.modifiedBanner;

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
      return { error: strings.rules.spendCapNotNumber };
    }
    if (parsed < 0) {
      return { error: strings.rules.spendCapNegative };
    }
    spendCapUsd = parsed;
  }
  if (approvalClasses.some((c) => c.trim() === "") || pathAllowlist.some((p) => p.trim() === "")) {
    return { error: strings.rules.emptyEntry };
  }
  return { policy: { spendCapUsd, approvalClasses, pathAllowlist } };
}

const panelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
  fontFamily: "var(--font-ui)",
};

const pathRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
};

const pathTextStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  wordBreak: "break-all",
};

/** Info banner (design-system.md "File-state banner" atom, task-17): the SAME inbox-item shape as
 * `DaemonBanner`'s incompatible case (left-edge accent + text + inline action) but with the calm
 * `--info` accent instead of `--warn` — amber is reserved for "needs you" (design-system.md §2),
 * and a stale/missing rules file is informational, not a human-attention gate. */
const bannerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-3)",
  padding: "var(--sp-2) var(--sp-3)",
  borderLeft: "3px solid var(--info)",
  background: "var(--info-weak)",
  color: "var(--ink)",
  fontSize: "var(--fs-md)",
  borderRadius: "var(--r-sm)",
};

const textareaStyle: CSSProperties = {
  width: "100%",
  minHeight: 200,
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

const policyFormStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  padding: "var(--sp-3) var(--sp-4)",
  borderRadius: "var(--r-lg)",
  background: "var(--panel)",
  boxShadow: "var(--shadow-1)",
};

const labelStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const numberInputStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "3px 6px",
  maxWidth: 140,
};

const errorTextStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--danger)",
};

const chipRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
};

const chipRemoveStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--muted)",
  cursor: "pointer",
  fontSize: "var(--fs-sm)",
  lineHeight: 1,
  padding: 0,
};

const chipInputStyle: CSSProperties = {
  flex: "1 1 140px",
  minWidth: 100,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
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
        <Badge key={v} data-testid={`${testIdPrefix}-chip-${v}`} tone="muted">
          {v}
          <button
            type="button"
            data-testid={`${testIdPrefix}-remove-${v}`}
            aria-label={strings.rules.deleteEntry(v)}
            onClick={() => onRemove(v)}
            style={chipRemoveStyle}
          >
            ×
          </button>
        </Badge>
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
      <Button
        type="button"
        variant="ghost"
        size="sm"
        data-testid={`${testIdPrefix}-add`}
        onClick={commitAdd}
        style={{ flexShrink: 0, whiteSpace: "nowrap" }}
      >
        {strings.rules.addEntry}
      </Button>
    </div>
  );
}

/**
 * Rules editor (S3 spec §7/§10, task-17). Markdown files are the source of truth — `mdContent`
 * here is only ever a DRAFT of the on-disk file, never authoritative until a Save round-trips.
 * `fileState` drives which affordances render: `ok` ⇒ a plain editable textarea, no banner;
 * `externallyModified` ⇒ the on-disk content (spec §7: "content returned") PLUS an info banner
 * offering [Accept] to accept the new hash without discarding the owner's ability to instead
 * just overwrite it with Save; `missing` ⇒ no textarea (there is no content to bind — `mdContent`
 * is `null`), only a banner offering [Recreate] (`UpsertRuleSet{mdContent: ""}`, spec §7's
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
 * (Save/Accept/Recreate/policy Save) so the UI reflects the new hash/state immediately
 * rather than waiting on a `RuleSetChanged` push that may not even fire for every one of these
 * (e.g. Acknowledge is a pure client action from the daemon's perspective in some races).
 *
 * Honest degradation (spec §10): while the store's `orchdDown` is `true`, every mutating button
 * (Save, Accept, Recreate, Save policy) is disabled — reads (the content
 * textarea, the policy draft fields, reveal file — a local Finder reveal, not an orchd
 * mutation) stay live. `ProjectPanel` owns the shared banner; this component only owns disabling
 * its own controls.
 */
export function RulesetPanel(props: { scope: RuleScope; projectId: string | null }): JSX.Element {
  const { scope, projectId } = props;
  const key = rulesetKey(scope, projectId);

  const view = useAppStore((s) => s.rulesets[key]);
  const refreshRuleset = useAppStore((s) => s.refreshRuleset);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

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
      <div data-testid="ruleset-panel-loading" style={{ color: "var(--muted)", fontSize: "var(--fs-md)" }}>
        {strings.rules.loading}
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
        <Button
          type="button"
          variant="ghost"
          size="sm"
          data-testid="ruleset-reveal"
          onClick={() => void handleReveal()}
          style={{ flexShrink: 0, whiteSpace: "nowrap" }}
        >
          {strings.rules.revealFile}
        </Button>
      </div>

      {fileState === "externallyModified" && (
        <div data-testid="ruleset-banner-modified" role="status" style={bannerStyle}>
          <span style={{ flex: 1 }}>{MODIFIED_BANNER_TEXT}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-testid="ruleset-acknowledge"
            disabled={orchdDown}
            onClick={() => void handleAcknowledge()}
            style={{ flexShrink: 0, whiteSpace: "nowrap" }}
          >
            {strings.common.accept}
          </Button>
        </div>
      )}

      {fileState === "missing" && (
        <div data-testid="ruleset-banner-missing" role="status" style={bannerStyle}>
          <span style={{ flex: 1 }}>{MISSING_BANNER_TEXT}</span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-testid="ruleset-recreate"
            disabled={orchdDown}
            onClick={() => void handleRecreate()}
            style={{ flexShrink: 0, whiteSpace: "nowrap" }}
          >
            {strings.rules.recreate}
          </Button>
        </div>
      )}

      {fileState !== "missing" && (
        <>
          <textarea
            data-testid="ruleset-content"
            aria-label={strings.rules.contentAria}
            value={content}
            onChange={(e) => setContent(e.target.value)}
            rows={12}
            style={textareaStyle}
          />
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="ruleset-save-content"
            disabled={orchdDown}
            onClick={() => void handleSaveContent()}
            style={{ alignSelf: "flex-start" }}
          >
            {strings.common.save}
          </Button>
        </>
      )}

      <div data-testid="ruleset-policy-form" style={policyFormStyle}>
        <span style={labelStyle}>{strings.rules.spendCapLabel}</span>
        <input
          data-testid="ruleset-spend-cap"
          aria-label={strings.rules.spendCapAria}
          type="number"
          placeholder={strings.rules.spendCapPlaceholder}
          value={spendCapText}
          onChange={(e) => {
            setSpendCapText(e.target.value);
            setPolicyError(null);
          }}
          style={numberInputStyle}
        />

        <span style={labelStyle}>{strings.rules.confirmClassesLabel}</span>
        <ChipList
          testIdPrefix="ruleset-approval-class"
          ariaLabel={strings.rules.confirmClassAria}
          placeholder={strings.rules.confirmClassPlaceholder}
          values={approvalClasses}
          onAdd={(v) => {
            setApprovalClasses((prev) => [...prev, v]);
            setPolicyError(null);
          }}
          onRemove={(v) => setApprovalClasses((prev) => prev.filter((c) => c !== v))}
        />

        <span style={labelStyle}>{strings.rules.allowedPathsLabel}</span>
        <ChipList
          testIdPrefix="ruleset-allowlist"
          ariaLabel={strings.rules.allowedPathAria}
          placeholder={strings.rules.allowedPathPlaceholder}
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

        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="ruleset-save-policy"
          disabled={orchdDown}
          onClick={() => void handleSavePolicy()}
          style={{ alignSelf: "flex-start" }}
        >
          {strings.rules.savePolicy}
        </Button>
      </div>
    </div>
  );
}
