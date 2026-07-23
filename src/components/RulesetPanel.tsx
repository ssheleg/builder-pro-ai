import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdUpsertRuleset,
  orchdAcknowledgeRuleFile,
  orchdRevealRulesFile,
  describeOrchdError,
} from "../ipc/orchd";
import type { PolicyRules, RuleScope, SupervisorConfig } from "../ipc/orchd-types";
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

/** Order-sensitive equality for the policy list drafts (approval classes, allowlist, delegated
 * classes, custom rules) — used by the dirty-draft guard to decide whether a field still matches
 * its last-hydrated baseline. */
function arrayEq(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((v, idx) => v === b[idx]);
}

/** SCN-046 "Recommended scope" preset (IMP-03/BP-012): the two safe classes it seeds into the CEO's
 * delegation scope. The caps stay whatever the policy already has — the preset never invents caps
 * (A-7: the CEO inherits the existing `spendCapUsd` + approval-class machinery), and the seeded
 * classes remain fully editable afterwards. */
const RECOMMENDED_SCOPE_CLASSES = ["safe-shell", "file-write"] as const;

/**
 * Client-side MIRROR of the server's `PolicyRules` strict validation (spec §5.2:
 * `deny_unknown_fields`, `spend_cap_usd >= 0`, non-empty allowlist/approval-class entries) —
 * blocks a doomed request before it round-trips, same discipline as `InsightsList`'s inline
 * archive-reasoning guard. `approvalClasses`/`pathAllowlist` can never actually contain an empty
 * entry here (the `ChipList` adder trims and refuses blanks), but this still re-checks them
 * defensively so the guard holds even if that invariant is ever broken upstream. A parseable but
 * negative cap is blocked; a non-numeric cap is blocked too (never silently coerced to 0/NaN).
 *
 * SCN-046: it also carries the CEO `supervisor` config into the built policy and enforces the core
 * invariant — an ENABLED CEO with an empty delegation scope is blocked with the "delegate at least
 * one class or disable the CEO" alert (the daemon's `validate_policy` is the authoritative twin).
 */
function validatePolicy(
  spendCapText: string,
  approvalClasses: string[],
  pathAllowlist: string[],
  supervisor: SupervisorConfig,
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
  if (supervisor.enabled && supervisor.delegatedClasses.length === 0) {
    return { error: strings.rules.supervisor.blockedNoClasses };
  }
  return { policy: { spendCapUsd, approvalClasses, pathAllowlist, supervisor } };
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
  // Tone edge as an inset shadow — a border-left under a radius renders as a curved wedge.
  boxShadow: "inset 3px 0 0 var(--info)",
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

// ── CEO supervisor section (SCN-046) styles ──────────────────────────────────────────────────

/** The supervisor block sits INSIDE the shared policy form (one "Save policy" saves both) but is
 * visually set off with a top divider so the CEO delegation reads as its own group. */
const supervisorSectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  marginTop: "var(--sp-2)",
  paddingTop: "var(--sp-3)",
  borderTop: "1px solid var(--line)",
};

const toggleRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  fontSize: "var(--fs-md)",
  color: "var(--ink)",
  cursor: "pointer",
};

const checkboxLabelStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: "var(--sp-1)",
  fontSize: "var(--fs-sm)",
  fontFamily: "var(--font-mono)",
  color: "var(--ink)",
  cursor: "pointer",
};

/** Muted informational lines: inherited-caps, info-access, scope summary, "MCP tools — soon". */
const mutedLineStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  lineHeight: 1.5,
};

const supervisorTextareaStyle: CSSProperties = {
  width: "100%",
  minHeight: 96,
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

/** Honesty-boundary pending note (S6b) — the calm `--info` inset-edge register of the file-state
 * banner, reused so "this is plumbing, not yet an acting CEO" reads as informational, never as a
 * "needs you" amber gate (design-system.md §2). Mirrors the Skills tab's registry banner role. */
const pendingNoteStyle: CSSProperties = {
  padding: "var(--sp-2) var(--sp-3)",
  boxShadow: "inset 3px 0 0 var(--info)",
  background: "var(--info-weak)",
  color: "var(--ink)",
  fontSize: "var(--fs-xs)",
  borderRadius: "var(--r-sm)",
  lineHeight: 1.5,
};

interface ChipListProps {
  testIdPrefix: string;
  ariaLabel: string;
  placeholder: string;
  values: string[];
  onAdd: (v: string) => void;
  onRemove: (v: string) => void;
}

/** Chip/list input for `approvalClasses`/`pathAllowlist`/CEO custom-rules (design-system.md "Policy
 * form" atom, task-17): existing entries render as Chip atoms with a `×` remove; a trimmed,
 * non-empty draft is added via the button or Enter — an empty/whitespace-only draft is a silent
 * no-op, so this widget can never itself produce the "empty entry" validation error `validatePolicy`
 * guards against. */
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
 *
 * CEO supervisor section (SCN-046, FLW-19, A-7) — project scope only (`scope === "project"`; the
 * global rules view has no CEO). It rides INSIDE the same policy form and is saved by the shared
 * "Save policy" button (one round-trip persists both the caps/lists AND the `supervisor` config, an
 * additive `PolicyRules` field). It progressively discloses (PRN-11): while the enable toggle is
 * OFF only the toggle, a muted "enable to configure" hint, and the S6b pending note render — the
 * delegation detail (delegated-class checkboxes, inherited-caps summary, "Recommended scope" preset
 * seeding safe-shell + file-write, instruction textarea, custom-rules editor, info-access + "CEO
 * may: …" scope summaries, "MCP tools — soon" placeholder) appears only once the CEO is enabled, so
 * a disabled CEO never reads as an active grant. `validatePolicy` blocks Save when the CEO is
 * enabled with an empty scope (SCN-046 "blocked alert"), the client twin of the daemon's
 * authoritative `validate_policy` guard.
 *
 * HONESTY BOUNDARY (S6b): this section is PLUMBING ONLY. Persisting the config never makes a CEO
 * act — the orchestrator-agent runtime that reads it and autonomously answers agent questions /
 * continues workflows (SCN-047/049) does not exist yet. The `pending` note states this in the same
 * register as the Skills tab's registry banner; nothing here starts an agent. Degradation (PRN-04):
 * while `orchdDown` the supervisor's interactive controls stay EDITABLE as ordinary drafts — the
 * same drafts-stay-live rule as the SCN-036 policy lists above; only "Save policy" is gated (the
 * shared correctness gate that protects the write), unified across the whole policy form.
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

  // ── CEO supervisor draft (SCN-046, A-7) — rides inside the same policy form, saved by the shared
  // "Save policy" button. Defaults to a disabled/empty CEO; hydrated from the policy's `supervisor`
  // field on every fresh view (the sync effect below). ──
  const [supervisorEnabled, setSupervisorEnabled] = useState<boolean>(
    view?.rule.policy.supervisor.enabled ?? false,
  );
  const [delegatedClasses, setDelegatedClasses] = useState<string[]>(
    view?.rule.policy.supervisor.delegatedClasses ?? [],
  );
  const [instruction, setInstruction] = useState<string>(view?.rule.policy.supervisor.instruction ?? "");
  const [customRules, setCustomRules] = useState<string[]>(view?.rule.policy.supervisor.customRules ?? []);

  // Dirty-draft guard baseline (PRN-03): the last policy values hydrated from the store, tagged
  // with the ruleset identity (`key` = scope+projectId). On a same-identity re-hydrate (an
  // `orchd://ruleset-changed` push or a reconnect rehydrate landing mid-edit) each field is only
  // re-adopted when its current draft still equals this baseline (not dirty) — an in-progress edit
  // is never silently clobbered. A change of identity (navigating to a different ruleset) always
  // hydrates fully. `null` until the first hydrate.
  const hydratedRef = useRef<{
    identity: string;
    spendCapText: string;
    approvalClasses: string[];
    pathAllowlist: string[];
    supervisorEnabled: boolean;
    delegatedClasses: string[];
    instruction: string;
    customRules: string[];
  } | null>(null);

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
  // Policy-draft hydration WITH the dirty-draft guard (PRN-03). See `hydratedRef` above: on a
  // same-identity update we keep any field the user has edited away from the last baseline and
  // only re-adopt the ones still matching it (clean); a new identity always hydrates in full.
  useEffect(() => {
    const policy = view?.rule.policy;
    const nextSpendCap = policy?.spendCapUsd == null ? "" : String(policy.spendCapUsd);
    const nextApprovalClasses = policy?.approvalClasses ?? [];
    const nextPathAllowlist = policy?.pathAllowlist ?? [];
    const nextSupervisorEnabled = policy?.supervisor.enabled ?? false;
    const nextDelegatedClasses = policy?.supervisor.delegatedClasses ?? [];
    const nextInstruction = policy?.supervisor.instruction ?? "";
    const nextCustomRules = policy?.supervisor.customRules ?? [];

    const baseline = hydratedRef.current;
    const sameIdentity = baseline !== null && baseline.identity === key;

    if (!sameIdentity) {
      setSpendCapText(nextSpendCap);
      setApprovalClasses(nextApprovalClasses);
      setPathAllowlist(nextPathAllowlist);
      setSupervisorEnabled(nextSupervisorEnabled);
      setDelegatedClasses(nextDelegatedClasses);
      setInstruction(nextInstruction);
      setCustomRules(nextCustomRules);
      setPolicyError(null);
    } else {
      setSpendCapText((cur) => (cur === baseline.spendCapText ? nextSpendCap : cur));
      setApprovalClasses((cur) => (arrayEq(cur, baseline.approvalClasses) ? nextApprovalClasses : cur));
      setPathAllowlist((cur) => (arrayEq(cur, baseline.pathAllowlist) ? nextPathAllowlist : cur));
      setSupervisorEnabled((cur) => (cur === baseline.supervisorEnabled ? nextSupervisorEnabled : cur));
      setDelegatedClasses((cur) => (arrayEq(cur, baseline.delegatedClasses) ? nextDelegatedClasses : cur));
      setInstruction((cur) => (cur === baseline.instruction ? nextInstruction : cur));
      setCustomRules((cur) => (arrayEq(cur, baseline.customRules) ? nextCustomRules : cur));
    }

    // Always advance the baseline to the latest server state so the next non-dirty hydrate works
    // and the external-change banner still reflects the newest hash.
    hydratedRef.current = {
      identity: key,
      spendCapText: nextSpendCap,
      approvalClasses: nextApprovalClasses,
      pathAllowlist: nextPathAllowlist,
      supervisorEnabled: nextSupervisorEnabled,
      delegatedClasses: nextDelegatedClasses,
      instruction: nextInstruction,
      customRules: nextCustomRules,
    };
    // `policy` is a fresh object every fetch — depending on its reference plus `key` catches both a
    // new GetRuleSet reply (same identity) and a navigation (identity change).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view?.rule.policy, key]);

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
    const supervisor: SupervisorConfig = {
      enabled: supervisorEnabled,
      delegatedClasses,
      instruction,
      customRules,
    };
    const result = validatePolicy(spendCapText, approvalClasses, pathAllowlist, supervisor);
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

  /** SCN-046 "Recommended scope" (IMP-03/BP-012): merge the two safe preset classes into the
   * delegation scope (dedup), leaving caps and everything else untouched and editable. */
  function applyRecommendedScope(): void {
    setDelegatedClasses((prev) =>
      Array.from(new Set([...prev, ...RECOMMENDED_SCOPE_CLASSES])),
    );
    setPolicyError(null);
  }

  /** Toggle one confirmation class in/out of the CEO delegation scope. */
  function toggleDelegatedClass(cls: string, checked: boolean): void {
    setDelegatedClasses((prev) =>
      checked ? Array.from(new Set([...prev, cls])) : prev.filter((c) => c !== cls),
    );
    setPolicyError(null);
  }

  // ── SCN-046 derived summaries (recomputed from live draft state so the summaries track edits
  // before Save). The delegation-checkbox universe is the union of the policy's confirmation
  // classes and any already-delegated classes (so preset-seeded classes appear even when they are
  // not in `approvalClasses`). Inherited caps read the LIVE spend-cap draft (A-7: the CEO inherits
  // the existing spend cap; per-project policy carries no calls/min, so none is shown). ──
  const delegationUniverse = Array.from(new Set([...approvalClasses, ...delegatedClasses]));
  const trimmedCapForSummary = spendCapText.trim();
  const inheritedCapLabel =
    trimmedCapForSummary === ""
      ? strings.rules.supervisor.inheritedNoSpendCap
      : strings.rules.supervisor.inheritedSpendCap(trimmedCapForSummary);
  const scopeSummaryClasses =
    delegatedClasses.length === 0
      ? strings.rules.supervisor.scopeSummaryNoClasses
      : delegatedClasses.join(", ");

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

        {scope === "project" && (
          <div data-testid="ruleset-supervisor" style={supervisorSectionStyle}>
            <span style={labelStyle}>{strings.rules.supervisor.sectionLabel}</span>

            <label style={toggleRowStyle}>
              <input
                type="checkbox"
                data-testid="ruleset-supervisor-enable"
                aria-label={strings.rules.supervisor.enableAria}
                checked={supervisorEnabled}
                onChange={(e) => {
                  setSupervisorEnabled(e.target.checked);
                  setPolicyError(null);
                }}
              />
              <span>{strings.rules.supervisor.enableLabel}</span>
            </label>

            {/* Progressive disclosure (PRN-11): the delegation/scope detail controls render ONLY
                when the CEO is enabled — a disabled CEO shows just this hint (plus the S6b pending
                note below), never a "CEO may: …" summary that would read as an active grant. */}
            {supervisorEnabled ? (
              <>
                <span style={labelStyle}>{strings.rules.supervisor.delegatedLabel}</span>
                {delegationUniverse.length === 0 ? (
                  <span data-testid="ruleset-supervisor-no-classes" style={mutedLineStyle}>
                    {strings.rules.supervisor.noClasses}
                  </span>
                ) : (
                  <div style={chipRowStyle} data-testid="ruleset-supervisor-classes">
                    {delegationUniverse.map((cls) => (
                      <label key={cls} style={checkboxLabelStyle}>
                        <input
                          type="checkbox"
                          data-testid={`ruleset-supervisor-class-${cls}`}
                          aria-label={strings.rules.supervisor.delegateClassAria(cls)}
                          checked={delegatedClasses.includes(cls)}
                          onChange={(e) => toggleDelegatedClass(cls, e.target.checked)}
                        />
                        <span>{cls}</span>
                      </label>
                    ))}
                  </div>
                )}

                <span data-testid="ruleset-supervisor-inherited-caps" style={mutedLineStyle}>
                  {strings.rules.supervisor.inheritedCapsLabel}: {inheritedCapLabel}
                </span>

                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid="ruleset-supervisor-recommended"
                  onClick={applyRecommendedScope}
                  style={{ alignSelf: "flex-start" }}
                >
                  {strings.rules.supervisor.recommendedScope}
                </Button>

                <span style={labelStyle}>{strings.rules.supervisor.instructionLabel}</span>
                <textarea
                  data-testid="ruleset-supervisor-instruction"
                  aria-label={strings.rules.supervisor.instructionAria}
                  placeholder={strings.rules.supervisor.instructionPlaceholder}
                  value={instruction}
                  onChange={(e) => {
                    setInstruction(e.target.value);
                    setPolicyError(null);
                  }}
                  rows={4}
                  style={supervisorTextareaStyle}
                />

                <span style={labelStyle}>{strings.rules.supervisor.customRulesLabel}</span>
                <ChipList
                  testIdPrefix="ruleset-supervisor-rule"
                  ariaLabel={strings.rules.supervisor.customRuleAria}
                  placeholder={strings.rules.supervisor.customRulePlaceholder}
                  values={customRules}
                  onAdd={(v) => {
                    setCustomRules((prev) => [...prev, v]);
                    setPolicyError(null);
                  }}
                  onRemove={(v) => setCustomRules((prev) => prev.filter((r) => r !== v))}
                />

                <span data-testid="ruleset-supervisor-info-access" style={mutedLineStyle}>
                  {strings.rules.supervisor.infoAccess}
                </span>
                <span data-testid="ruleset-supervisor-scope-summary" style={mutedLineStyle}>
                  {strings.rules.supervisor.scopeSummary(scopeSummaryClasses, inheritedCapLabel)}
                </span>
                <span data-testid="ruleset-supervisor-mcp-soon" style={mutedLineStyle}>
                  {strings.rules.supervisor.mcpSoon}
                </span>
              </>
            ) : (
              <span data-testid="ruleset-supervisor-disabled-hint" style={mutedLineStyle}>
                {strings.rules.supervisor.disabledHint}
              </span>
            )}

            <div data-testid="ruleset-supervisor-pending" role="note" style={pendingNoteStyle}>
              {strings.rules.supervisor.pendingNote}
            </div>
          </div>
        )}

        {policyError !== null && (
          <span data-testid="ruleset-policy-error" role="alert" style={errorTextStyle}>
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
