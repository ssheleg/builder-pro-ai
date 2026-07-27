//! `skill` CRUD (S-EXT spec §4 schema v3, task T17). Builds directly on `persistence::Db`'s
//! `conn()` seam plus its `now_ms`/`OrchdPersistError` — exactly like `mcp::registry` builds on
//! `persistence` (see that module's doc comment) — PLUS the file-handling primitives this table
//! additionally needs: `md_path` validation (no symlink-escape, must be a real file),
//! sha256-hashing the file's bytes into `md_hash`, and a minimal SKILL.md frontmatter parser for
//! `name`/`description` (Q14 — see `skills`'s module doc comment).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::persistence::{now_ms, Db, OrchdPersistError};

use super::{NewSkill, SkillFileState, SkillRow, SkillScope};

// ---- skill.scope <-> TEXT helpers (spec §4 CHECK literal) ----

fn encode_scope(s: &SkillScope) -> &'static str {
    match s {
        SkillScope::Global => "global",
        SkillScope::Project => "project",
    }
}

fn decode_scope(s: &str) -> Result<SkillScope, OrchdPersistError> {
    match s {
        "global" => Ok(SkillScope::Global),
        "project" => Ok(SkillScope::Project),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt skill.scope value: {other}"
        ))),
    }
}

/// Validates the spec §4 CHECK invariant on `skill` in Rust BEFORE the insert, so a caller gets a
/// typed `Validation` error rather than a raw SQLite `ConstraintViolation` — mirrors
/// `mcp::registry::validate_new_server`'s scope⇄project_id half exactly (`skill` has no
/// transport⇄url analogue, so only the one check applies here).
fn validate_new_skill_scope(new: &NewSkill) -> Result<(), OrchdPersistError> {
    match (&new.scope, &new.project_id) {
        (SkillScope::Project, None) => Err(OrchdPersistError::Validation(
            "skill: scope='project' requires project_id".to_string(),
        )),
        (SkillScope::Global, Some(_)) => Err(OrchdPersistError::Validation(
            "skill: scope='global' requires project_id to be absent".to_string(),
        )),
        _ => Ok(()),
    }
}

/// Validates `md_path` (task-17 brief): must be an ABSOLUTE path to a real, existing FILE whose
/// canonical form does not escape its own lexical parent directory via a symlink. Reuses
/// `bpa_paths::validate_path_within` with the path's OWN parent directory as `root` — the
/// narrowest possible containment check for a single file (mirrors how `bpa_paths` itself is used
/// for workspace-root/session-cwd validation elsewhere in this codebase, just scoped down from "a
/// directory tree" to "one file's immediate parent"). Returns the canonicalized path on success;
/// every failure mode (relative path, no parent, parent/file missing, symlink escape, a directory
/// rather than a regular file) maps to a typed `Validation` error — never a panic, never a raw
/// `std::io::Error` leaking through.
fn validate_md_path(md_path: &str) -> Result<PathBuf, OrchdPersistError> {
    let path = Path::new(md_path);
    if !path.is_absolute() {
        return Err(OrchdPersistError::Validation(
            "skill md_path must be absolute".to_string(),
        ));
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let parent = match parent {
        Some(p) => p,
        None => {
            return Err(OrchdPersistError::Validation(
                "skill md_path has no parent directory".to_string(),
            ))
        }
    };
    let canonical = bpa_paths::validate_path_within(parent, path)
        .map_err(|e| OrchdPersistError::Validation(format!("skill md_path is invalid: {e}")))?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|e| OrchdPersistError::Validation(format!("skill md_path cannot be read: {e}")))?;
    if !meta.is_file() {
        return Err(OrchdPersistError::Validation(
            "skill md_path must be a regular file, not a directory".to_string(),
        ));
    }
    Ok(canonical)
}

/// Minimal SKILL.md YAML-frontmatter parser (Q14 — see `skills`'s module doc comment): a leading
/// `---` line, then `key: value` scalar lines, then a closing `---` line. Only the `name`/
/// `description` keys are extracted (every other key, and anything richer than a scalar `key:
/// value` line — nested maps, lists, multiline strings — is out of scope for this "minimal
/// frontmatter parser", task-17 brief). Returns `(None, None)` when `content` has no frontmatter
/// block at all (its very first line is not `---`) — a SKILL.md with no frontmatter is not an
/// error at this layer, just a file this parser cannot fill `name`/`description` in from
/// (`add_skill` is the one that turns "no name available from ANY source" into `Validation`).
fn parse_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    match lines.next() {
        Some(first) if first.trim() == "---" => {}
        _ => return (None, None),
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            match key {
                "name" if !value.is_empty() => name = Some(value),
                "description" if !value.is_empty() => description = Some(value),
                _ => {}
            }
        }
    }
    (name, description)
}

/// Files-as-truth read-time classification (task-17 brief: "computed by re-reading the file at
/// list time"), shared by `Db::list_skills` and `SkillRow::into_view` so both paths agree on
/// exactly one way to decide `Present`/`Modified`/`Missing` — mirrors
/// `ruleset_files::read_state`'s classification logic, minus returning the file content (the
/// skills UI only needs the tri-state badge, never the raw markdown — see `SkillView`'s own doc
/// comment). Any read failure (missing file, permission denied, a directory now sitting at
/// `md_path`, non-UTF8 content, …) folds into `Missing` — this crate has nothing more specific and
/// actionable to report than "the file this row points at cannot be read right now" (same
/// honest-degradation stance `ruleset_files::read_state` documents for itself).
pub fn compute_file_state(md_path: &str, stored_hash: &str) -> SkillFileState {
    // BL-77: an oversized file folds into `Missing` (cannot be read safely right now) instead of
    // being buffered into memory wholesale — mirrors `ruleset_files::read_state`.
    if crate::ruleset_files::exceeds_read_cap(std::path::Path::new(md_path)) {
        return SkillFileState::Missing;
    }
    match std::fs::read_to_string(md_path) {
        Ok(content) => {
            if crate::ruleset_files::sha256_hex(&content) == stored_hash {
                SkillFileState::Present
            } else {
                SkillFileState::Modified
            }
        }
        Err(_) => SkillFileState::Missing,
    }
}

fn load_skill(conn: &Connection, id: &str) -> Result<SkillRow, OrchdPersistError> {
    let raw = conn
        .query_row(
            "SELECT id, name, description, md_path, md_hash, scope, project_id, created_at, updated_at
             FROM skill WHERE id = ?1",
            rusqlite::params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?;
    let (id, name, description, md_path, md_hash, scope, project_id, created_at, updated_at) = raw;
    Ok(SkillRow {
        id,
        name,
        description,
        md_path,
        md_hash,
        scope: decode_scope(&scope)?,
        project_id,
        created_at,
        updated_at,
    })
}

impl Db {
    /// `add_skill` (task-17 brief): validates `md_path` ([`validate_md_path`] — rejects a
    /// relative path, a missing file, a symlink-escaping path, or a directory with `Validation`),
    /// reads it, computes `md_hash = sha256(bytes)` hex (via `ruleset_files::sha256_hex`, the
    /// crate's ONE hashing implementation — see that function's own doc comment), and parses its
    /// frontmatter ([`parse_frontmatter`]). `new.name`/`new.description` take priority when
    /// provided; a frontmatter value fills in only when the caller left the field `None`. If NO
    /// name is available from either source, the insert never happens — `Validation("name
    /// required")`. Enforces the scope⇄project_id invariant ([`validate_new_skill_scope`]) before
    /// touching the filesystem at all, mirrors `mcp::registry::add_mcp_server`'s "validate first"
    /// discipline.
    pub fn add_skill(&self, new: NewSkill) -> Result<SkillRow, OrchdPersistError> {
        validate_new_skill_scope(&new)?;
        let canonical_path = validate_md_path(&new.md_path)?;
        // BL-77: refuse an oversized SKILL.md at add time rather than buffering it into memory.
        if crate::ruleset_files::exceeds_read_cap(&canonical_path) {
            return Err(OrchdPersistError::Validation(format!(
                "skill md_path exceeds the {} byte read cap",
                crate::ruleset_files::MAX_MD_READ_BYTES
            )));
        }
        let content = std::fs::read_to_string(&canonical_path).map_err(|e| {
            OrchdPersistError::Validation(format!("skill md_path cannot be read: {e}"))
        })?;
        let md_hash = crate::ruleset_files::sha256_hex(&content);
        let (fm_name, fm_description) = parse_frontmatter(&content);

        let name = new
            .name
            .filter(|n| !n.is_empty())
            .or(fm_name)
            .ok_or_else(|| {
                OrchdPersistError::Validation(
                    "skill: name required (pass it explicitly or via the SKILL.md frontmatter)"
                        .to_string(),
                )
            })?;
        let description = new
            .description
            .filter(|d| !d.is_empty())
            .or(fm_description)
            .unwrap_or_default();

        let tx = self.conn().unchecked_transaction()?;
        // DOM-6 of the 2026-07-24 audit remediation: a project-scoped skill must reference an
        // EXISTING, ACTIVE project — an unknown `project_id` previously leaked as a raw FK
        // `OrchdPersistError::Sql`, and an archived one was silently accepted (spec §5.2's
        // archived doctrine applies to every mutating verb touching a project's children).
        crate::persistence::ensure_optional_project_active(&tx, new.project_id.as_deref())?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        let md_path_string = canonical_path.to_string_lossy().to_string();
        tx.execute(
            "INSERT INTO skill
               (id, name, description, md_path, md_hash, scope, project_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                id,
                name,
                description,
                md_path_string,
                md_hash,
                encode_scope(&new.scope),
                new.project_id,
                now,
            ],
        )?;
        let row = load_skill(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `get_skill` (needed by `socket_server`'s `SkillDelete` dispatch arm, mirrors
    /// `mcp::registry::get_mcp_server`'s exact role: the single-row lookup a delete verb uses to
    /// learn the row's `project_id` BEFORE removing it, so it can broadcast an accurately-scoped
    /// `SkillsChanged{project_id}` push). Unknown `id` ⇒ `NotFound`.
    pub fn get_skill(&self, id: &str) -> Result<SkillRow, OrchdPersistError> {
        load_skill(self.conn(), id)
    }

    /// `list_skills` (task-17 brief): `Some(project_id)` returns global-scope skills PLUS that
    /// project's own; `None` returns global-scope skills only — mirrors
    /// `mcp::registry::list_mcp_servers`'s scoping query exactly. Each returned [`super::SkillView`]
    /// carries a FRESH files-as-truth read ([`SkillRow::into_view`]) — never a cached/stale status.
    pub fn list_skills(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<super::SkillView>, OrchdPersistError> {
        let mut stmt = self.conn().prepare(
            "SELECT id FROM skill
             WHERE scope = 'global' OR (?1 IS NOT NULL AND scope = 'project' AND project_id = ?1)
             ORDER BY created_at, id",
        )?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![project_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter()
            .map(|id| load_skill(self.conn(), id).map(SkillRow::into_view))
            .collect()
    }

    /// `delete_skill` (task-17 brief): removes the DB row only — this registry never touches the
    /// SKILL.md file itself on disk (files-as-truth: the file's lifecycle is the owner's, not
    /// orchd's, mirroring RuleSet's own "never deletes the markdown file" discipline). Unknown
    /// `id` ⇒ `NotFound`; a skill belonging to an ARCHIVED project ⇒ `Invariant` (DOM-6 of the
    /// 2026-07-24 audit remediation).
    pub fn delete_skill(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let project_id: Option<Option<String>> = tx
            .query_row(
                "SELECT project_id FROM skill WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?;
        let project_id = project_id.ok_or(OrchdPersistError::NotFound)?;
        crate::persistence::ensure_optional_project_active(&tx, project_id.as_deref())?;
        tx.execute("DELETE FROM skill WHERE id = ?1", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_project(db: &Db) -> String {
        // `project_workspace.workspace_id` is UNIQUE across the whole table (S3 spec §5.2) — a
        // fresh uuid per call so multi-project tests don't collide (mirrors `mcp::registry`'s own
        // test helper of the same name).
        let workspace_id = Uuid::new_v4().to_string();
        db.create_project("P", "", &[workspace_id]).unwrap().id
    }

    /// Writes a SKILL.md with a full frontmatter block (`name`/`description`) plus a body, under
    /// a fresh tempdir, and returns `(TempDir, absolute path)` — the `TempDir` guard must be kept
    /// alive by the caller for as long as the path is used (dropping it deletes the directory).
    fn write_skill_md(content: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        std::fs::write(&path, content).unwrap();
        let path_str = path.to_string_lossy().to_string();
        (dir, path_str)
    }

    const WITH_FRONTMATTER: &str = "---\nname: Frontmatter Skill\ndescription: parsed from the frontmatter\n---\n\n# Body\n\nDoes a thing.\n";
    const NO_FRONTMATTER: &str = "# Just a heading\n\nNo frontmatter here.\n";

    fn new_skill(md_path: &str) -> NewSkill {
        NewSkill {
            name: None,
            description: None,
            md_path: md_path.to_string(),
            scope: SkillScope::Global,
            project_id: None,
        }
    }

    // ---- add_skill ----

    #[test]
    fn add_skill_with_explicit_name_sets_row_and_md_hash() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.name = Some("Explicit Name".to_string());
        new.description = Some("explicit description".to_string());

        let row = db.add_skill(new).unwrap();
        assert_eq!(
            row.name, "Explicit Name",
            "explicit name wins over frontmatter"
        );
        assert_eq!(row.description, "explicit description");
        // `add_skill` stores the CANONICALIZED path (`validate_md_path`'s return), which on macOS
        // may differ byte-for-byte from the raw tempdir path (`/var/...` is itself a symlink to
        // `/private/var/...`) — compare canonical forms, not raw strings.
        assert_eq!(
            Path::new(&row.md_path),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(!row.md_hash.is_empty());
        assert_eq!(
            row.md_hash,
            crate::ruleset_files::sha256_hex(WITH_FRONTMATTER)
        );
        assert_eq!(row.scope, SkillScope::Global);
        assert_eq!(row.project_id, None);
        assert!(!row.id.is_empty());
        assert_eq!(row.created_at, row.updated_at);
    }

    #[test]
    fn add_skill_with_no_name_parses_name_and_description_from_frontmatter() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);

        let row = db.add_skill(new_skill(&path)).unwrap();
        assert_eq!(row.name, "Frontmatter Skill");
        assert_eq!(row.description, "parsed from the frontmatter");
    }

    #[test]
    fn add_skill_with_no_name_and_no_frontmatter_is_validation() {
        let db = new_db();
        let (_guard, path) = write_skill_md(NO_FRONTMATTER);

        let err = db.add_skill(new_skill(&path)).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_frontmatter_with_only_description_still_requires_a_name() {
        let db = new_db();
        let (_guard, path) = write_skill_md("---\ndescription: no name in here\n---\n\nbody\n");

        let err = db.add_skill(new_skill(&path)).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_symlink_escaping_md_path_is_validation() {
        // layout: base/outside/SKILL.md (real file, OUTSIDE `named`); base/named/SKILL.md is a
        // SYMLINK to that outside file — its canonical form resolves to `base/outside`, which is
        // not a descendant of its own lexical parent `base/named` (mirrors `bpa_paths`'s own
        // `symlink_pointing_outside_root_is_rejected` fixture, adapted to a file target).
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        let named = base.path().join("named");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&named).unwrap();
        std::fs::write(outside.join("SKILL.md"), WITH_FRONTMATTER).unwrap();
        let link = named.join("SKILL.md");
        std::os::unix::fs::symlink(outside.join("SKILL.md"), &link).unwrap();

        let db = new_db();
        let err = db
            .add_skill(new_skill(&link.to_string_lossy()))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_directory_md_path_is_validation() {
        let dir = tempfile::tempdir().unwrap();
        let db = new_db();
        let err = db
            .add_skill(new_skill(&dir.path().to_string_lossy()))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_relative_md_path_is_validation() {
        let db = new_db();
        let err = db.add_skill(new_skill("relative/SKILL.md")).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_missing_md_path_is_validation() {
        let dir = tempfile::tempdir().unwrap();
        let db = new_db();
        let gone = dir.path().join("does-not-exist.md");
        let err = db
            .add_skill(new_skill(&gone.to_string_lossy()))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_scope_project_without_project_id_is_validation() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.scope = SkillScope::Project;
        let err = db.add_skill(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_scope_global_with_project_id_is_validation() {
        let db = new_db();
        let project_id = new_project(&db);
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.project_id = Some(project_id);
        let err = db.add_skill(new).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn add_skill_project_scope_round_trips() {
        let db = new_db();
        let project_id = new_project(&db);
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.scope = SkillScope::Project;
        new.project_id = Some(project_id.clone());

        let row = db.add_skill(new).unwrap();
        assert_eq!(row.scope, SkillScope::Project);
        assert_eq!(row.project_id.as_deref(), Some(project_id.as_str()));
    }

    // ---- list_skills / files-as-truth ----

    #[test]
    fn list_skills_file_state_is_present_right_after_add() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        db.add_skill(new_skill(&path)).unwrap();

        let views = db.list_skills(None).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].file_state, SkillFileState::Present);
    }

    #[test]
    fn list_skills_file_state_is_modified_after_the_file_changes_on_disk() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        db.add_skill(new_skill(&path)).unwrap();

        // Someone edits the file directly on disk, bypassing this registry entirely.
        std::fs::write(
            &path,
            "---\nname: Frontmatter Skill\ndescription: changed\n---\nedited\n",
        )
        .unwrap();

        let views = db.list_skills(None).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].file_state, SkillFileState::Modified);
    }

    #[test]
    fn list_skills_file_state_is_missing_after_the_file_is_deleted() {
        let db = new_db();
        let (guard, path) = write_skill_md(WITH_FRONTMATTER);
        db.add_skill(new_skill(&path)).unwrap();

        std::fs::remove_file(&path).unwrap();

        let views = db.list_skills(None).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].file_state, SkillFileState::Missing);
        drop(guard);
    }

    #[test]
    fn list_skills_returns_global_plus_own_project_not_other_projects() {
        let db = new_db();
        let project_a = new_project(&db);
        let project_b = new_project(&db);

        let (_g1, global_path) = write_skill_md(WITH_FRONTMATTER);
        let global = db.add_skill(new_skill(&global_path)).unwrap();

        let (_g2, a_path) = write_skill_md(WITH_FRONTMATTER);
        let mut a_new = new_skill(&a_path);
        a_new.scope = SkillScope::Project;
        a_new.project_id = Some(project_a.clone());
        let a_skill = db.add_skill(a_new).unwrap();

        let (_g3, b_path) = write_skill_md(WITH_FRONTMATTER);
        let mut b_new = new_skill(&b_path);
        b_new.scope = SkillScope::Project;
        b_new.project_id = Some(project_b.clone());
        db.add_skill(b_new).unwrap();

        let for_a = db.list_skills(Some(&project_a)).unwrap();
        let ids: Vec<&str> = for_a.iter().map(|v| v.skill.id.as_str()).collect();
        assert!(ids.contains(&global.id.as_str()));
        assert!(ids.contains(&a_skill.id.as_str()));
        assert_eq!(for_a.len(), 2, "must not include project B's skill");

        let none_ctx = db.list_skills(None).unwrap();
        assert_eq!(
            none_ctx.len(),
            1,
            "no project context ⇒ global-scope skills only"
        );
        assert_eq!(none_ctx[0].skill.id, global.id);
    }

    // ---- delete_skill ----

    #[test]
    fn delete_skill_removes_the_row_but_never_the_file() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let row = db.add_skill(new_skill(&path)).unwrap();

        db.delete_skill(&row.id).unwrap();

        assert!(db.list_skills(None).unwrap().is_empty());
        assert!(
            Path::new(&path).exists(),
            "delete_skill must never touch the SKILL.md file on disk"
        );
    }

    #[test]
    fn delete_skill_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.delete_skill("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ---- DOM-6 (2026-07-24 audit remediation): archived-project guard + typed existence
    // precheck on the skill registry mutators ----

    #[test]
    fn add_skill_on_an_archived_project_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        db.archive_project(&project_id).unwrap();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.scope = SkillScope::Project;
        new.project_id = Some(project_id);

        let err = db.add_skill(new).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Invariant(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn add_skill_with_a_bogus_project_id_is_typed_not_found() {
        let db = new_db();
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.scope = SkillScope::Project;
        new.project_id = Some("no-such-project".to_string());

        let err = db.add_skill(new).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::NotFound),
            "a bogus project_id must be the typed NotFound, never a raw FK Sql error: {err:?}"
        );
    }

    #[test]
    fn delete_skill_on_an_archived_projects_skill_is_invariant() {
        let db = new_db();
        let project_id = new_project(&db);
        let (_guard, path) = write_skill_md(WITH_FRONTMATTER);
        let mut new = new_skill(&path);
        new.scope = SkillScope::Project;
        new.project_id = Some(project_id.clone());
        let row = db.add_skill(new).unwrap();
        db.archive_project(&project_id).unwrap();

        let err = db.delete_skill(&row.id).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Invariant(_)),
            "got {err:?}"
        );
    }
}
