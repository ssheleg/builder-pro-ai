import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { pickSkillFile } from "../../ipc/commands";
import { skillAdd, skillDelete, describeOrchdError } from "../../ipc/orchd";
import type { Skill, SkillFileState } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { Badge, Button, Input, Select, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

/** Honest message for a rejected `CommandError` (`pickSkillFile` is a sessiond native-picker
 * round-trip — `src-tauri/src/commands.rs::CommandError`, a DIFFERENT error union than orchd's).
 * `describeOrchdError` maps a sessiond error's `kind:"internal"` to the generic "unknown
 * orchestrator error", losing the real message (P-16) — this mapper preserves it. Deliberately
 * duplicated per-surface, mirroring `FileTree.tsx`/`WorkspaceSidebar.tsx`. */
function describeCommandError(err: unknown): string {
  const e = err as { kind?: string; message?: string; code?: string; reason?: string } | undefined;
  switch (e?.kind) {
    case "daemon":
      return e.message ?? e.code ?? strings.errors.command.daemon;
    case "disconnected":
      return strings.errors.command.disconnected;
    case "internal":
      return e.message ?? strings.errors.command.internal;
    case "incompatibleDaemon":
      return strings.errors.command.incompatible;
    case "upgradeFailed":
      return e.reason ?? strings.errors.command.failed;
    case "tooLarge":
      return strings.errors.command.tooLarge;
    default:
      return err instanceof Error ? err.message : String(err);
  }
}

const bannerStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  lineHeight: 1.5,
  color: "var(--warn)",
  background: "var(--warn-weak)",
  border: "1px solid var(--warn)",
  borderRadius: "var(--r-md)",
  padding: "var(--sp-2) var(--sp-3)",
  marginBottom: "var(--sp-4)",
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
  padding: "var(--sp-3)",
  marginBottom: "var(--sp-3)",
  borderRadius: "var(--r-md)",
  background: "var(--panel-2)",
};

const createInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
};

const selectStyle: CSSProperties = {
  flexShrink: 0,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
  padding: "var(--sp-2)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  borderBottom: "1px solid var(--hairline)",
};

const titleTextStyle: CSSProperties = {
  minWidth: 0,
  color: "var(--ink)",
  fontWeight: 600,
};

const metaStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-xs)",
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const pathTextStyle: CSSProperties = {
  ...metaStyle,
  flex: "1 1 200px",
};

/** Files-as-truth badge copy (task-17 brief: "modified"/"file missing" for Modified/Missing).
 * `present` renders no badge at all — nothing wrong to flag, mirrors `ConnectorsTab`'s "only show
 * a banner when there's something to say" discipline. */
const FILE_STATE_LABEL: Partial<Record<SkillFileState, string>> = {
  modified: strings.ext.skills.badge.modified,
  missing: strings.ext.skills.badge.missing,
};

const SCOPE_LABEL: Record<Skill["scope"], string> = {
  global: strings.common.scope.global,
  project: strings.common.scope.project,
};

/**
 * Skills tab (S-EXT §8, D11, Q14, task T17): the SKILL.md skills registry — list + add
 * (pick-a-SKILL.md-file) + remove. PLUMBING ONLY (D11): there is no runtime consumer of this
 * registry yet — the banner below says so honestly, never presenting the list as something that
 * currently executes anything.
 *
 * Mirrors `ServersTab`/`ConnectorsTab`'s conventions exactly: on mount `refreshSkills()`, every
 * mutating control `disabled={orchdDown}`, every async failure -> `showToast(describeOrchdError(e))`
 * rather than a silent no-op. The add form's `scope` picker is fixed at `"global"` (a `"project"`
 * option is present but disabled, "soon") — this top-level Extensions view carries no
 * per-project context to scope a `"project"` skill to, same rationale `ServersTab`'s own scope
 * picker documents for MCP servers.
 *
 * `md_path` is chosen via the native file picker (`pickSkillFile`, `src-tauri/src/commands.rs`),
 * never typed free-hand — the owner picks an EXISTING SKILL.md; `skillAdd`'s own `md_path`
 * validation (symlink-escape / non-file rejection) is the actual security boundary, this is just
 * the honest UX for "point at a file that already exists".
 */
export function SkillsTab(): JSX.Element {
  const skills = useAppStore((s) => s.skills);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshSkills = useAppStore((s) => s.refreshSkills);
  const showToast = useAppStore((s) => s.showToast);
  const { submitting, guard } = useSubmitGuard();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [mdPath, setMdPath] = useState<string | null>(null);

  useEffect(() => {
    void refreshSkills();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addBlocked = mdPath === null;

  async function handlePickFile(): Promise<void> {
    try {
      const path = await pickSkillFile();
      if (path === null) return; // cancelled -> no-op, mirrors CreateProjectDialog's pickFolder
      setMdPath(path);
    } catch (e) {
      // `pickSkillFile` is a sessiond `CommandError`, NOT an orchd error — `describeCommandError`
      // keeps its real message instead of the generic orchd fallback `describeOrchdError` gives it.
      showToast(describeCommandError(e));
    }
  }

  async function handleAdd(): Promise<void> {
    if (mdPath === null) return;
    try {
      await skillAdd(
        name.trim() === "" ? null : name.trim(),
        description.trim() === "" ? null : description.trim(),
        mdPath,
        "global",
        null,
      );
      setName("");
      setDescription("");
      setMdPath(null);
      await refreshSkills();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second "+ skill" click must NOT register the same
  // SKILL.md twice (cross-cutting P-19).
  const submitAdd = guard(handleAdd);

  async function handleDelete(skill: Skill): Promise<void> {
    if (!window.confirm(strings.ext.skills.deleteConfirm(skill.name))) return;
    try {
      await skillDelete(skill.id);
      await refreshSkills();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  return (
    <div data-testid="skills-tab">
      <div data-testid="skills-banner" role="status" style={bannerStyle}>
        {strings.ext.skills.registryBanner}
      </div>

      <div style={createFormStyle}>
        <Input
          data-testid="skill-create-name"
          aria-label={strings.ext.skills.nameAria}
          placeholder={strings.ext.skills.namePlaceholder}
          value={name}
          onChange={(e) => setName(e.target.value)}
          style={createInputStyle}
        />
        <Input
          data-testid="skill-create-description"
          aria-label={strings.ext.skills.descriptionAria}
          placeholder={strings.common.descriptionOptional}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          style={createInputStyle}
        />
        <Select
          data-testid="skill-create-scope"
          aria-label={strings.ext.skills.scopeAria}
          value="global"
          disabled
          style={selectStyle}
        >
          <option value="global">{strings.common.scope.global}</option>
          <option value="project" disabled>
            {strings.ext.projectSoon}
          </option>
        </Select>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          data-testid="skill-pick-path"
          onClick={() => void handlePickFile()}
        >
          {strings.ext.skills.chooseSkillMd}
        </Button>
        {mdPath !== null && (
          <span data-testid="skill-picked-path" style={pathTextStyle} title={mdPath}>
            {mdPath}
          </span>
        )}
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="skill-create-submit"
          disabled={orchdDown || addBlocked || submitting}
          onClick={() => void submitAdd()}
        >
          {strings.ext.skills.addSkill}
        </Button>
      </div>

      {skills.length === 0 ? (
        <EmptyState data-testid="skills-empty" title={strings.ext.skills.empty} />
      ) : (
        <div role="list">
          {skills.map((skill) => {
            const badgeLabel = FILE_STATE_LABEL[skill.fileState];
            return (
              <div
                key={skill.id}
                data-testid={`skill-row-${skill.id}`}
                role="listitem"
                style={rowStyle}
              >
                <span data-testid={`skill-name-${skill.id}`} style={titleTextStyle}>
                  {skill.name}
                </span>
                <span data-testid={`skill-description-${skill.id}`} style={metaStyle}>
                  {skill.description}
                </span>
                <span
                  data-testid={`skill-path-${skill.id}`}
                  style={pathTextStyle}
                  title={skill.mdPath}
                >
                  {skill.mdPath}
                </span>
                <span data-testid={`skill-scope-${skill.id}`} style={metaStyle}>
                  {SCOPE_LABEL[skill.scope]}
                </span>
                {badgeLabel !== undefined && (
                  <Badge tone="danger" data-testid={`skill-filestate-${skill.id}`}>
                    {badgeLabel}
                  </Badge>
                )}
                <Button
                  type="button"
                  variant="danger"
                  size="sm"
                  data-testid={`skill-delete-${skill.id}`}
                  disabled={orchdDown}
                  onClick={() => void handleDelete(skill)}
                >
                  {strings.ext.delete}
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
