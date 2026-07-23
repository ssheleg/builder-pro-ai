//! Export / import (spec §8, D7): `bundleFormat: 1` JSON bundles — per-project and whole-store —
//! with field-verbatim preservation on import (ids, timestamps, `rank`/`ord`/`md_hash` copied
//! exactly, never re-stamped) and single-transaction, collision-safe atomicity (`Conflict` +
//! full rollback on any PK/UNIQUE hit). Ruleset markdown content is read LIVE at export time via
//! `ruleset_files::read_state` and, on import, written ONLY under the caller-supplied
//! `app_support` root — a foreign `md_path` is never touched; the file lands at the scope's
//! default app-support path instead, and the row is repointed there.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use bpa_orchd_proto::{DomainTask, Goal, Idea, Insight, Project, RuleScope, RuleSet};
use bpa_protocol::MAX_FRAME_LEN;

use crate::persistence::{self, Db, OrchdPersistError};
use crate::ruleset_files;

/// Locked bundle format version (spec §8, D7). The ONLY value [`import_bundle`] accepts.
const BUNDLE_FORMAT: u32 = 1;

/// Headroom subtracted from `MAX_FRAME_LEN` for the CBOR frame envelope wrapping the exported
/// JSON string on the wire (spec §8 "Size cap"): the export only needs to leave SOME margin
/// rather than compute the envelope's exact byte overhead.
const FRAME_CAP_MARGIN: usize = 1024;

/// Per-family row counts an `ImportBundle` request actually inserted (spec §4.2:
/// `OrchdResponse::ImportReport` has the identical field set — the dispatch layer builds that
/// reply straight from this struct).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub projects: u32,
    pub goals: u32,
    pub ideas: u32,
    pub insights: u32,
    pub tasks: u32,
    pub rulesets: u32,
}

/// `{ "rule": RuleSet, "mdContent": "…" } | null` (spec §8). `mdContent: null` means the file was
/// missing (or unreadable) when read live; an EMPTY file exports as `""`. No separate
/// `mdMissing` flag — `null` IS the missing signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleSetBundle {
    rule: RuleSet,
    md_content: Option<String>,
}

/// The `project`/`goals`/`ideas`/`insights`/`tasks`/`ruleset` fields shared verbatim by BOTH
/// locked bundle shapes (spec §8): [`export_project`]'s top-level object (`bundleFormat`/
/// `exportedAt` flattened in alongside this, see [`ProjectExportEnvelope`]) AND each element of
/// [`export_all`]'s `projects[]` array, which — per spec — is exactly this shape with NOTHING
/// flattened in ("per-project bundle objects, without `bundleFormat`/`exportedAt`").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectBundle {
    project: Project,
    goals: Vec<Goal>,
    ideas: Vec<Idea>,
    insights: Vec<Insight>,
    tasks: Vec<DomainTask>,
    ruleset: Option<RuleSetBundle>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectExportEnvelope {
    bundle_format: u32,
    exported_at: i64,
    #[serde(flatten)]
    bundle: ProjectBundle,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AllExportEnvelope {
    bundle_format: u32,
    exported_at: i64,
    projects: Vec<ProjectBundle>,
    global_ruleset: Option<RuleSetBundle>,
    orphan_ideas: Vec<Idea>,
    orphan_insights: Vec<Insight>,
}

/// Deserialize-only counterpart of [`AllExportEnvelope`] — [`import_bundle`] has already
/// validated `bundleFormat` against the raw `serde_json::Value` before deserializing into this,
/// so this struct doesn't need those two fields at all (any present in the input JSON are simply
/// ignored, same as every other struct in this module — none of them use
/// `deny_unknown_fields`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllImportEnvelope {
    projects: Vec<ProjectBundle>,
    #[serde(default)]
    global_ruleset: Option<RuleSetBundle>,
    #[serde(default)]
    orphan_ideas: Vec<Idea>,
    #[serde(default)]
    orphan_insights: Vec<Insight>,
}

/// Reads `md_path` fresh (spec §7/§8: export never uses a cache) and keeps only the content half
/// of `ruleset_files::read_state`'s `(content, state)` pair — export's `mdContent` field doesn't
/// distinguish `Missing` from any other unreadable state, only "was there content or not" (spec
/// §8: "missing file ⇒ `mdContent: null`... No separate `mdMissing` flag").
fn read_live_md_content(md_path: &str, stored_hash: &str) -> Option<String> {
    ruleset_files::read_state(Path::new(md_path), stored_hash).0
}

/// Builds a `{ rule, mdContent }` bundle for `(scope, project_id)`, or `None` if no ruleset row
/// exists at all for that key. Every project's ruleset row is auto-created with the project
/// (spec §5.2) so `None` shouldn't happen for `RuleScope::Project` in practice, but the global
/// row is only ensured at daemon BOOT (`boot::ensure_global_ruleset`) — a DB that was never
/// booted (every test in this module, which opens `Db::open_in_memory()` directly) genuinely has
/// no global ruleset row, so `RuleScope::Global` hitting this `NotFound` branch is the common
/// case, not a defensive dead end.
fn build_ruleset_bundle(
    db: &Db,
    scope: RuleScope,
    project_id: Option<&str>,
) -> Result<Option<RuleSetBundle>, OrchdPersistError> {
    match db.get_ruleset(scope, project_id) {
        Ok(rule) => {
            let md_content = read_live_md_content(&rule.md_path, &rule.md_hash);
            Ok(Some(RuleSetBundle { rule, md_content }))
        }
        Err(OrchdPersistError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Assembles one project's full bundle: the project row, its goals (tree order, via
/// `Db::list_goals`), its ideas/insights (orphans excluded — `Some(project_id)` scopes both
/// lists to just this project), its tasks, and its ruleset (with live-read `mdContent`).
fn build_project_bundle(db: &Db, project_id: &str) -> Result<ProjectBundle, OrchdPersistError> {
    let project = db.get_project(project_id)?;
    let goals = db.list_goals(project_id)?;
    let ideas = db.list_ideas(Some(project_id))?;
    let insights = db.list_insights(Some(project_id))?;
    let tasks = db.list_tasks(Some(project_id))?;
    let ruleset = build_ruleset_bundle(db, RuleScope::Project, Some(project_id))?;
    Ok(ProjectBundle {
        project,
        goals,
        ideas,
        insights,
        tasks,
        ruleset,
    })
}

/// Serializes `value` to a JSON string, then enforces the §8 frame-cap guard (`MAX_FRAME_LEN`
/// minus [`FRAME_CAP_MARGIN`]) BEFORE returning it — an oversized reply is never handed back for
/// the dispatch layer to attempt sending (which would fail anyway, less honestly, deeper in the
/// framing layer).
fn serialize_with_frame_cap<T: Serialize>(value: &T) -> Result<String, OrchdPersistError> {
    let json = serde_json::to_string(value)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize export bundle: {e}")))?;
    let cap = (MAX_FRAME_LEN as usize).saturating_sub(FRAME_CAP_MARGIN);
    if json.len() > cap {
        return Err(OrchdPersistError::Io(
            "export exceeds the 16 MiB frame cap".to_string(),
        ));
    }
    Ok(json)
}

/// `ExportProject` (spec §8): one project's full state as a standalone `bundleFormat: 1` JSON
/// bundle. `exported_at` is caller-supplied (unix-ms) — this module never calls a clock itself;
/// the dispatch layer stamps it. `mdContent` is read LIVE, not from any cache (see
/// [`read_live_md_content`]). Unknown `project_id` ⇒ `NotFound`.
pub fn export_project(
    db: &Db,
    project_id: &str,
    exported_at: i64,
) -> Result<String, OrchdPersistError> {
    let bundle = build_project_bundle(db, project_id)?;
    let envelope = ProjectExportEnvelope {
        bundle_format: BUNDLE_FORMAT,
        exported_at,
        bundle,
    };
    serialize_with_frame_cap(&envelope)
}

/// `ExportAll` (spec §8): every project's bundle, the global ruleset, and orphan ideas/insights
/// (`projectId: null`), as one `bundleFormat: 1` JSON bundle.
pub fn export_all(db: &Db, exported_at: i64) -> Result<String, OrchdPersistError> {
    let projects = db.list_projects()?;
    let mut project_bundles = Vec::with_capacity(projects.len());
    for project in &projects {
        project_bundles.push(build_project_bundle(db, &project.id)?);
    }
    let global_ruleset = build_ruleset_bundle(db, RuleScope::Global, None)?;
    // `list_ideas`/`list_insights(None)` return EVERY row including project-linked ones (spec
    // §4.2 "`project_id: None` ⇒ ALL... incl. orphans") — filter down to just the orphans
    // (`projectId: null`) for this field; project-linked ideas/insights already live inside
    // their own project's bundle above.
    let orphan_ideas = db
        .list_ideas(None)?
        .into_iter()
        .filter(|i| i.project_id.is_none())
        .collect();
    let orphan_insights = db
        .list_insights(None)?
        .into_iter()
        .filter(|i| i.project_id.is_none())
        .collect();

    let envelope = AllExportEnvelope {
        bundle_format: BUNDLE_FORMAT,
        exported_at,
        projects: project_bundles,
        global_ruleset,
        orphan_ideas,
        orphan_insights,
    };
    serialize_with_frame_cap(&envelope)
}

/// A bundle's `project_id`, when interpolated into a `project-<id>.md` path segment
/// ([`default_ruleset_md_path`]), MUST be a single plain path segment — a real one is a UUID.
/// Import bundles are UNTRUSTED input, so reject any `..` occurrence or path separator (`/`, `\`,
/// or the platform separator), which is exactly what would let a crafted `project_id` like
/// `x/../../etc/passwd` escape the app-support tree. Empty is rejected too (would collapse the
/// filename to `project-.md` and is never a legit id).
fn validate_project_id_segment(project_id: &str) -> Result<(), OrchdPersistError> {
    let bad = project_id.is_empty()
        || project_id.contains("..")
        || project_id.contains('/')
        || project_id.contains('\\')
        || project_id.contains(std::path::MAIN_SEPARATOR);
    if bad {
        return Err(OrchdPersistError::Validation(format!(
            "ruleset project_id is not a plain path segment: {project_id:?}"
        )));
    }
    Ok(())
}

/// Default app-support-relative `md_path` for a ruleset that needs (re)pointing during import
/// (spec §8: "otherwise the import writes to the default app-support path and repoints"):
/// mirrors `boot::ensure_global_ruleset`'s `{app_support}/rules/global.md` and
/// `persistence::project_ruleset_md_path`'s `{app_support}/rules/project-<id>.md` shapes, just
/// parameterized on the CALLER-supplied `app_support` root instead of the real
/// `bpa_daemon_core::dirs::app_support_dir()` — import must never touch the real app-support
/// tree from a test, and in production `app_support` IS that real root (spec §8, D6 pattern).
/// The `project_id` is validated ([`validate_project_id_segment`]) BEFORE interpolation so a
/// crafted id can never traverse out of `{app_support}/rules/`.
fn default_ruleset_md_path(
    app_support: &Path,
    scope: &RuleScope,
    project_id: Option<&str>,
) -> Result<PathBuf, OrchdPersistError> {
    let rules_dir = app_support.join("rules");
    match (scope, project_id) {
        (RuleScope::Global, _) => Ok(rules_dir.join("global.md")),
        (RuleScope::Project, Some(pid)) => {
            validate_project_id_segment(pid)?;
            Ok(rules_dir.join(format!("project-{pid}.md")))
        }
        // A project-scope ruleset with no project_id violates the DB CHECK constraint
        // (`scope='project'` ⇒ `project_id IS NOT NULL`) — a malformed bundle, rejected fail-closed
        // rather than invented into a `project-unknown.md` path.
        (RuleScope::Project, None) => Err(OrchdPersistError::Validation(
            "project-scope ruleset is missing its project_id".to_string(),
        )),
    }
}

/// Resolves the FINAL on-disk path a ruleset's `mdContent` will be written to during import,
/// fail-closed against path traversal (CRITICAL: import bundles are untrusted user input).
///
/// The bundle's own `md_path` is honored verbatim ONLY if it is absolute AND lexically contained
/// under `app_support`; otherwise it is repointed to the scope's default app-support path. The
/// containment decision below (`starts_with`) is purely LEXICAL — it does NOT resolve `..` — so a
/// crafted `md_path` like `{app_support}/../../../../etc/cron.d/evil` would "start with"
/// `app_support` and escape. Guard against that up front by rejecting ANY `..`
/// ([`Component::ParentDir`]) component in the bundle's `md_path`; only then is `starts_with`
/// sound. This is a lexical check (no filesystem access), which is exactly what the not-yet-
/// written import target needs.
fn resolve_ruleset_write_path(
    app_support: &Path,
    rule: &RuleSet,
) -> Result<PathBuf, OrchdPersistError> {
    let bundle_path = Path::new(&rule.md_path);
    if bundle_path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(OrchdPersistError::Validation(
            "ruleset md_path escapes app-support (contains '..')".to_string(),
        ));
    }
    if bundle_path.is_absolute() && bundle_path.starts_with(app_support) {
        Ok(bundle_path.to_path_buf())
    } else {
        default_ruleset_md_path(app_support, &rule.scope, rule.project_id.as_deref())
    }
}

/// One ruleset markdown file write deferred to AFTER the import transaction commits (BL-90). A
/// file write cannot be rolled back with the DB transaction, so writing it inside the tx left an
/// orphan `.md` on disk whenever a LATER bundle in the same import failed on a `Conflict`. The
/// path is fully validated (traversal + symlink guard) at COLLECTION time — before commit — so a
/// crafted `md_path` still fails the whole import before anything is written; only the atomic
/// content write itself is deferred.
struct PendingRulesetWrite {
    path: std::path::PathBuf,
    content: String,
}

/// Validates a ruleset's write path (spec §8 ruleset-file rule: write only under `app_support`,
/// otherwise repoint to the default app-support path), inserts its raw row inside the already-open
/// import transaction, and — when there is `mdContent` — QUEUES the file write into `pending` for
/// execution after commit (BL-90). `md_content: None` (file was missing/unreadable at export time)
/// queues nothing; the row is still inserted, and `md_hash` is preserved verbatim regardless (this
/// function never re-stamps `md_hash`, per D7 field-verbatim preservation).
///
/// SECURITY: the write path is resolved fail-closed by [`resolve_ruleset_write_path`] (rejects
/// `..`), then — as defense-in-depth against a symlink planted inside the app-support tree — the
/// realized parent directory is created and canonicalized-and-re-checked to be contained within
/// `app_support` via the shared `bpa_paths::validate_path_within` validator. This full check runs
/// HERE (pre-commit) so a traversal/symlink attempt fails the whole import before any row commits
/// and before any file is written; the deferred step is purely the content write.
fn import_ruleset(
    tx: &rusqlite::Transaction,
    app_support: &Path,
    bundle: &RuleSetBundle,
    pending: &mut Vec<PendingRulesetWrite>,
) -> Result<(), OrchdPersistError> {
    let mut rule = bundle.rule.clone();
    let effective_path = resolve_ruleset_write_path(app_support, &rule)?;

    if let Some(content) = &bundle.md_content {
        // Create the (lexically-contained) parent, then canonicalize-and-contain it: this catches
        // any symlink escape the lexical `..` check can't see. Done here (pre-commit) so a bad
        // path fails the whole import before commit — but the actual content write is deferred to
        // after commit so a later `Conflict` can't leave an orphan file (BL-90).
        let parent = effective_path.parent().ok_or_else(|| {
            OrchdPersistError::Validation("ruleset md_path has no parent directory".to_string())
        })?;
        std::fs::create_dir_all(parent)
            .map_err(|e| OrchdPersistError::Io(format!("failed to create rules dir: {e}")))?;
        bpa_paths::validate_path_within(app_support, parent).map_err(|e| {
            OrchdPersistError::Validation(format!("ruleset md_path escapes app-support: {e}"))
        })?;
        pending.push(PendingRulesetWrite {
            path: effective_path.clone(),
            content: content.clone(),
        });
    }
    rule.md_path = effective_path.to_string_lossy().into_owned();
    persistence::insert_ruleset_raw(tx, &rule)
}

/// Raw-inserts one project bundle's rows (project → its goals/ideas/insights/tasks → its
/// ruleset) and bumps `counts`. Insertion order here is for readability only — FK enforcement is
/// deferred for the WHOLE import transaction by [`import_project_bundles`], so a bundle array
/// that isn't parent-before-child (e.g. a reranked subtask sorting ahead of its parent in
/// `tasks[]`) still imports correctly.
fn import_one_project(
    tx: &rusqlite::Transaction,
    app_support: &Path,
    bundle: &ProjectBundle,
    counts: &mut ImportCounts,
    pending: &mut Vec<PendingRulesetWrite>,
) -> Result<(), OrchdPersistError> {
    persistence::insert_project_raw(tx, &bundle.project)?;
    counts.projects += 1;
    for goal in &bundle.goals {
        persistence::insert_goal_raw(tx, goal)?;
        counts.goals += 1;
    }
    for idea in &bundle.ideas {
        persistence::insert_idea_raw(tx, idea)?;
        counts.ideas += 1;
    }
    for insight in &bundle.insights {
        persistence::insert_insight_raw(tx, insight)?;
        counts.insights += 1;
    }
    for task in &bundle.tasks {
        persistence::insert_task_raw(tx, task)?;
        counts.tasks += 1;
    }
    if let Some(ruleset) = &bundle.ruleset {
        import_ruleset(tx, app_support, ruleset, pending)?;
        counts.rulesets += 1;
    }
    Ok(())
}

/// Core of [`import_bundle`] once the two locked shapes have been normalized down to their
/// shared parts: a list of project bundles, an optional global ruleset, and orphan
/// ideas/insights. ONE transaction (spec §8): any PK/UNIQUE hit anywhere ⇒ `Conflict`, and
/// nothing survives — `tx` is simply dropped without `commit()` on the `?` early return, which
/// rolls the whole transaction back (the same pattern every multi-statement verb in
/// `persistence.rs` already relies on, e.g. `Db::create_project`'s workspace-conflict rollback).
fn import_project_bundles(
    db: &Db,
    app_support: &Path,
    project_bundles: &[ProjectBundle],
    global_ruleset: Option<&RuleSetBundle>,
    orphan_ideas: &[Idea],
    orphan_insights: &[Insight],
) -> Result<ImportCounts, OrchdPersistError> {
    let tx = db.conn().unchecked_transaction()?;
    // See `import_one_project`'s doc: bundle arrays are not guaranteed FK-dependency order.
    // Deferring FK enforcement to COMMIT (SQLite semantics; auto-resets to OFF at COMMIT/
    // ROLLBACK, so it never leaks into a later transaction on this connection) means every
    // `insert_*_raw` call below can run in the bundle's own array order and still fail closed on
    // a genuinely dangling reference at commit time, instead of this module having to
    // topologically re-sort every family first.
    tx.pragma_update(None, "defer_foreign_keys", "ON")?;

    // Ruleset markdown writes are COLLECTED here (path fully validated) and executed only after a
    // successful commit (BL-90) — a file write cannot roll back with the transaction, so writing
    // inside the tx left an orphan `.md` whenever a later bundle hit a `Conflict`.
    let mut counts = ImportCounts::default();
    let mut pending_writes: Vec<PendingRulesetWrite> = Vec::new();
    for bundle in project_bundles {
        import_one_project(&tx, app_support, bundle, &mut counts, &mut pending_writes)?;
    }
    if let Some(ruleset) = global_ruleset {
        import_ruleset(&tx, app_support, ruleset, &mut pending_writes)?;
        counts.rulesets += 1;
    }
    for idea in orphan_ideas {
        persistence::insert_idea_raw(&tx, idea)?;
        counts.ideas += 1;
    }
    for insight in orphan_insights {
        persistence::insert_insight_raw(&tx, insight)?;
        counts.insights += 1;
    }

    tx.commit()?;

    // Commit succeeded and is durable — NOW write the ruleset files. If a write fails here the row
    // is already committed (the file is best-effort recreatable via the ruleset editor), so surface
    // it but the import has otherwise landed.
    for write in &pending_writes {
        ruleset_files::write_atomic(&write.path, &write.content)
            .map_err(|e| OrchdPersistError::Io(e.to_string()))?;
    }
    Ok(counts)
}

/// `ImportBundle` (spec §8, D7): parses `json`, validates `bundleFormat == 1` (else
/// `Validation`), discriminates the two locked shapes on the presence of a `project` (per-
/// project) vs `projects` (whole-store) top-level key, then raw-inserts every row inside ONE
/// transaction. Every row field is written EXACTLY as parsed (ids, timestamps, `rank`/`ord`/
/// `md_hash`) — this function never re-stamps anything. Ruleset markdown is written ONLY under
/// `app_support`; a foreign `md_path` is repointed to the scope's default app-support path
/// instead (spec §8).
pub fn import_bundle(
    db: &Db,
    app_support: &Path,
    json: &str,
) -> Result<ImportCounts, OrchdPersistError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| OrchdPersistError::Validation(format!("invalid import JSON: {e}")))?;

    let bundle_format = value
        .get("bundleFormat")
        .and_then(serde_json::Value::as_u64);
    if bundle_format != Some(u64::from(BUNDLE_FORMAT)) {
        return Err(OrchdPersistError::Validation(format!(
            "unsupported bundleFormat (expected {BUNDLE_FORMAT}): {:?}",
            value.get("bundleFormat")
        )));
    }

    if value.get("project").is_some() {
        let bundle: ProjectBundle = serde_json::from_value(value)
            .map_err(|e| OrchdPersistError::Validation(format!("invalid project bundle: {e}")))?;
        import_project_bundles(
            db,
            app_support,
            std::slice::from_ref(&bundle),
            None,
            &[],
            &[],
        )
    } else if value.get("projects").is_some() {
        let envelope: AllImportEnvelope = serde_json::from_value(value)
            .map_err(|e| OrchdPersistError::Validation(format!("invalid store bundle: {e}")))?;
        import_project_bundles(
            db,
            app_support,
            &envelope.projects,
            envelope.global_ruleset.as_ref(),
            &envelope.orphan_ideas,
            &envelope.orphan_insights,
        )
    } else {
        Err(OrchdPersistError::Validation(
            "import bundle has neither a 'project' nor a 'projects' key".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_orchd_proto::{FitVerdict, GoalKind, TaskSource};

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Every table this module's raw inserts can touch, plus `project_workspace` — used by the
    /// collision test to prove a rolled-back import left EVERY table untouched, not just the one
    /// that collided.
    const TABLES: &[&str] = &[
        "project",
        "project_workspace",
        "goal",
        "idea",
        "insight",
        "task",
        "ruleset",
    ];

    fn table_counts(db: &Db) -> Vec<i64> {
        TABLES
            .iter()
            .map(|t| {
                db.conn()
                    .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                    .unwrap()
            })
            .collect()
    }

    struct Fixture {
        project_id: String,
        subtask_id: String,
    }

    /// Builds "a project with 2-level goals + orphan idea + subtask + ruleset content" (task-9
    /// brief's round-trip fixture), plus one insight (fit-verdict set) for full family coverage.
    /// The project's ruleset content is written under `ruleset_root` — the caller decides whether
    /// that's the SAME root it later passes as `import_bundle`'s `app_support` (path preserved
    /// verbatim) or a DIFFERENT one (path gets repointed on import), depending on what the test
    /// is proving. `workspace_id` is caller-chosen (not hard-coded) so a test that builds a
    /// SECOND fixture in another DB and imports it alongside a first project can pick a distinct
    /// id — `project_workspace.workspace_id` is globally UNIQUE, so two projects reusing the same
    /// literal would collide there instead of on whatever this test is actually trying to prove.
    fn build_fixture(db: &Db, ruleset_root: &Path, workspace_id: &str) -> Fixture {
        let project = db
            .create_project("Proj A", "", &ids(&[workspace_id]))
            .unwrap();

        let strategic = db
            .list_goals(&project.id)
            .unwrap()
            .into_iter()
            .find(|g| g.kind == GoalKind::Strategic)
            .expect("create_project auto-creates a strategic goal");
        let child = db
            .create_goal(
                &project.id,
                Some(&strategic.id),
                GoalKind::Additional,
                "Child goal",
                "",
            )
            .unwrap();
        db.create_goal(
            &project.id,
            Some(&child.id),
            GoalKind::Additional,
            "Grandchild goal",
            "",
        )
        .unwrap();

        db.create_idea(None, "Orphan idea", "orphan body").unwrap();

        let insight = db
            .create_insight(Some(&project.id), "manual", "Insight", "insight body")
            .unwrap();
        db.set_insight_fit_verdict(&insight.id, Some(FitVerdict::Fit), "good fit")
            .unwrap();

        let parent_task = db
            .create_task(
                &project.id,
                None,
                "Parent task",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let subtask = db
            .create_task(
                &project.id,
                Some(&parent_task.id),
                "Subtask",
                "",
                None,
                TaskSource::Plan,
                None,
                &["tag1".to_string()],
                None,
            )
            .unwrap();

        let rules_dir = ruleset_root.join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();
        let md_path = rules_dir.join(format!("project-{}.md", project.id));
        db.upsert_ruleset(
            RuleScope::Project,
            Some(&project.id),
            Some("# rules\ncontent\n"),
            Some(md_path.to_str().unwrap()),
            None,
        )
        .unwrap();

        Fixture {
            project_id: project.id,
            subtask_id: subtask.id,
        }
    }

    // ---- round trip ----

    #[test]
    fn export_import_round_trip_is_semantically_identical_modulo_exported_at() {
        let db = Db::open_in_memory().unwrap();
        // ONE tempdir plays both roles: the fixture's ruleset content is written under it, and
        // it's also passed as `import_bundle`'s `app_support` — so the path is "under app
        // support" on import and is preserved verbatim, which is required for the re-export to
        // come out byte-for-byte (well, Value-for-Value) identical to the original.
        let root = tempfile::tempdir().unwrap();
        build_fixture(&db, root.path(), "w1");

        let exported = export_all(&db, 1_700_000_000_000).unwrap();

        let fresh = Db::open_in_memory().unwrap();
        let counts = import_bundle(&fresh, root.path(), &exported).unwrap();
        assert_eq!(counts.projects, 1);
        assert_eq!(counts.goals, 3);
        assert_eq!(counts.ideas, 1);
        assert_eq!(counts.insights, 1);
        assert_eq!(counts.tasks, 2);
        assert_eq!(counts.rulesets, 1);

        let re_exported = export_all(&fresh, 1_800_000_000_000).unwrap();

        let mut original: serde_json::Value = serde_json::from_str(&exported).unwrap();
        let mut roundtripped: serde_json::Value = serde_json::from_str(&re_exported).unwrap();
        original.as_object_mut().unwrap().remove("exportedAt");
        roundtripped.as_object_mut().unwrap().remove("exportedAt");

        assert_eq!(
            original, roundtripped,
            "import into an empty store then re-export must equal the original modulo exportedAt"
        );
    }

    #[test]
    fn import_preserves_updated_at_and_rank_verbatim_not_freshly_stamped() {
        let db = Db::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let fixture = build_fixture(&db, root.path(), "w1");

        // Force the subtask's `updated_at`/`rank` to distinctive, clearly-not-"now" values so
        // "verbatim" is actually falsifiable: if import re-stamped either, this could not survive
        // unchanged. `now_ms()` at test time is ~1.7e12+; 1_000_000_000_000 (Sept 2001) is safely
        // distinguishable from it.
        let past = 1_000_000_000_000_i64;
        let distinctive_rank = 42.5_f64;
        db.conn()
            .execute(
                "UPDATE task SET updated_at = ?2, rank = ?3 WHERE id = ?1",
                rusqlite::params![fixture.subtask_id, past, distinctive_rank],
            )
            .unwrap();

        let exported = export_all(&db, 1).unwrap();

        let fresh = Db::open_in_memory().unwrap();
        let import_root = tempfile::tempdir().unwrap();
        import_bundle(&fresh, import_root.path(), &exported).unwrap();

        let reimported_task = fresh
            .list_tasks(None)
            .unwrap()
            .into_iter()
            .find(|t| t.id == fixture.subtask_id)
            .expect("subtask must have been imported");

        assert_eq!(
            reimported_task.updated_at, past,
            "updated_at must be copied verbatim, not now()"
        );
        assert_eq!(
            reimported_task.rank, distinctive_rank,
            "rank must be copied verbatim"
        );

        // The project row itself round-trips its own updated_at verbatim too, not just tasks.
        let original_project = db.get_project(&fixture.project_id).unwrap();
        let reimported_project = fresh.get_project(&fixture.project_id).unwrap();
        assert_eq!(
            reimported_project.updated_at, original_project.updated_at,
            "project.updated_at must be copied verbatim"
        );
        assert_eq!(reimported_project.created_at, original_project.created_at);
    }

    // ---- collision ----

    #[test]
    fn import_task_id_collision_is_conflict_and_rolls_back_everything() {
        let target = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        let existing_project = target
            .create_project("Existing", "", &ids(&["w1"]))
            .unwrap();
        let existing_task = target
            .create_task(
                &existing_project.id,
                None,
                "Existing task",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let before = table_counts(&target);

        // A FOREIGN bundle (its own project/goal/etc, built via a separate staging DB + export)
        // whose task id is patched to collide with `existing_task.id` — every OTHER field stays
        // realistic/valid, only the id is forced to collide.
        let staging = Db::open_in_memory().unwrap();
        let staging_root = tempfile::tempdir().unwrap();
        // Distinct workspace id from `existing_project`'s "w1" — `project_workspace.workspace_id`
        // is globally UNIQUE, so reusing "w1" here would collide there first and this test would
        // never reach the task-id collision it's actually trying to prove.
        let staging_fixture = build_fixture(&staging, staging_root.path(), "w2");
        let foreign_json = export_project(&staging, &staging_fixture.project_id, 1).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&foreign_json).unwrap();
        value["tasks"][0]["id"] = serde_json::Value::String(existing_task.id.clone());
        let patched = serde_json::to_string(&value).unwrap();

        let err = import_bundle(&target, app_support.path(), &patched).unwrap_err();
        match err {
            OrchdPersistError::Conflict(msg) => {
                assert!(
                    msg.contains(existing_task.id.as_str()),
                    "expected the colliding task's own id in the message, got {msg:?}"
                );
                assert!(msg.contains("already exists"));
            }
            other => panic!("expected Conflict, got {other:?}"),
        }

        assert_eq!(
            before,
            table_counts(&target),
            "a Conflict must roll back EVERY row the doomed import attempted, not just the \
             colliding one"
        );
    }

    /// BL-90 regression: a ruleset markdown file must NOT survive on disk when a LATER row in the
    /// same import fails on a `Conflict`. A file write cannot roll back with the DB transaction, so
    /// the ruleset `.md` write is deferred to after commit. A whole-store bundle imports a project
    /// WITH a ruleset (queueing a `.md` write), then an orphan idea whose id is forced to collide
    /// with an existing target idea — imported AFTER every project bundle — so the rollback must
    /// leave NO orphan `.md` behind.
    #[test]
    fn import_conflict_leaves_no_orphan_ruleset_file() {
        // Staging: one project (build_fixture creates its ruleset + mdContent) plus one orphan idea.
        let staging = Db::open_in_memory().unwrap();
        let staging_support = tempfile::tempdir().unwrap();
        let fixture = build_fixture(&staging, staging_support.path(), "w1");
        staging.create_idea(None, "Orphan idea", "").unwrap();
        let exported = export_all(&staging, 1).unwrap();

        // Target: pre-create an orphan idea, then force the bundle's orphan idea id to collide.
        let target = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        let existing_idea = target.create_idea(None, "Existing orphan", "").unwrap();

        let mut value: serde_json::Value = serde_json::from_str(&exported).unwrap();
        value["orphanIdeas"][0]["id"] = serde_json::Value::String(existing_idea.id.clone());
        let patched = serde_json::to_string(&value).unwrap();

        // The project's ruleset `.md` would be written at this repointed path.
        let expected_md = app_support
            .path()
            .join("rules")
            .join(format!("project-{}.md", fixture.project_id));

        let err = import_bundle(&target, app_support.path(), &patched).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Conflict(_)),
            "expected Conflict, got {err:?}"
        );

        assert!(
            !expected_md.exists(),
            "a rolled-back import must not leave an orphan ruleset file at {}",
            expected_md.display()
        );
        // The DB is fully rolled back too — the imported project never lands.
        assert!(
            target.get_project(&fixture.project_id).is_err(),
            "the rolled-back import must not leave the project either"
        );
    }

    // ---- ruleset md_path repoint ----

    #[test]
    fn import_repoints_a_foreign_ruleset_md_path_under_the_given_app_support() {
        let staging = Db::open_in_memory().unwrap();
        // Deliberately NOT the same root as the import's app_support below.
        let foreign_root = tempfile::tempdir().unwrap();
        let fixture = build_fixture(&staging, foreign_root.path(), "w1");
        let json = export_project(&staging, &fixture.project_id, 1).unwrap();

        let target = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        import_bundle(&target, app_support.path(), &json).unwrap();

        let imported_ruleset = target
            .get_ruleset(RuleScope::Project, Some(&fixture.project_id))
            .unwrap();
        let imported_path = Path::new(&imported_ruleset.md_path);

        assert!(
            imported_path.starts_with(app_support.path()),
            "a foreign md_path must be repointed under the import's app_support root, got {}",
            imported_ruleset.md_path
        );
        assert!(
            !imported_path.starts_with(foreign_root.path()),
            "the foreign path itself must never be reused"
        );
        assert_eq!(
            std::fs::read_to_string(imported_path).unwrap(),
            "# rules\ncontent\n",
            "the exported mdContent must land at the repointed path"
        );
    }

    // ---- bundleFormat validation ----

    #[test]
    fn import_rejects_an_unsupported_bundle_format() {
        let db = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        let json = r#"{"bundleFormat":2,"exportedAt":0,"project":{},"goals":[],"ideas":[],
                        "insights":[],"tasks":[],"ruleset":null}"#;

        let err = import_bundle(&db, app_support.path(), json).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)));
        assert_eq!(table_counts(&db).iter().sum::<i64>(), 0);
    }

    #[test]
    fn import_rejects_json_missing_a_project_or_projects_key() {
        let db = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        let json = r#"{"bundleFormat":1,"exportedAt":0}"#;

        let err = import_bundle(&db, app_support.path(), json).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    // ---- standalone per-project bundle ----

    #[test]
    fn import_accepts_a_standalone_per_project_bundle() {
        let staging = Db::open_in_memory().unwrap();
        let staging_root = tempfile::tempdir().unwrap();
        let fixture = build_fixture(&staging, staging_root.path(), "w1");
        let json = export_project(&staging, &fixture.project_id, 1).unwrap();

        let target = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();
        let counts = import_bundle(&target, app_support.path(), &json).unwrap();

        assert_eq!(counts.projects, 1);
        assert_eq!(counts.goals, 3);
        // The fixture's one idea is an ORPHAN (project_id: null) — a per-project bundle's
        // `ideas[]` only ever contains that project's OWN ideas (spec §8), so it's excluded here;
        // orphans only travel through `export_all`'s top-level `orphanIdeas[]` (see the round-trip
        // and verbatim tests above, which use `export_all` and do see `counts.ideas == 1`).
        assert_eq!(counts.ideas, 0);
        assert_eq!(counts.insights, 1);
        assert_eq!(counts.tasks, 2);
        assert_eq!(counts.rulesets, 1);
        assert_eq!(target.list_projects().unwrap().len(), 1);
    }

    // ---- oversize / frame cap ----

    #[test]
    fn export_project_over_16_mib_is_an_io_frame_cap_error() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("Big", "", &ids(&["w1"])).unwrap();
        let huge_body = "x".repeat(17 * 1024 * 1024);
        db.create_idea(Some(&project.id), "huge idea", &huge_body)
            .unwrap();

        let err = export_project(&db, &project.id, 1).unwrap_err();
        match err {
            OrchdPersistError::Io(msg) => assert!(msg.contains("16 MiB frame cap")),
            other => panic!("expected Io frame-cap error, got {other:?}"),
        }
    }

    #[test]
    fn export_all_over_16_mib_is_an_io_frame_cap_error() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("Big", "", &ids(&["w1"])).unwrap();
        let huge_body = "y".repeat(17 * 1024 * 1024);
        db.create_insight(Some(&project.id), "manual", "huge insight", &huge_body)
            .unwrap();

        let err = export_all(&db, 1).unwrap_err();
        match err {
            OrchdPersistError::Io(msg) => assert!(msg.contains("16 MiB frame cap")),
            other => panic!("expected Io frame-cap error, got {other:?}"),
        }
    }

    // ---- ruleset mdContent null / empty ----

    #[test]
    fn export_project_ruleset_with_a_missing_file_has_null_md_content() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("P", "", &ids(&["w1"])).unwrap();
        // create_project's auto-created ruleset row's md_hash is "" and its file was never
        // written — read_state must report Missing (None), never an error. This never writes
        // anywhere (only `read_to_string`s a path that doesn't exist), so it stays hermetic even
        // though the row's default md_path resolves under the REAL app-support tree.
        let exported = export_project(&db, &project.id, 1).unwrap();
        let value: serde_json::Value = serde_json::from_str(&exported).unwrap();

        assert_eq!(value["ruleset"]["mdContent"], serde_json::Value::Null);
        assert!(
            value["ruleset"].get("mdMissing").is_none(),
            "no separate mdMissing flag"
        );
        assert!(value["ruleset"]["rule"].is_object());
    }

    #[test]
    fn export_project_ruleset_with_an_empty_file_has_empty_string_md_content_not_null() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("P", "", &ids(&["w1"])).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("rules.md");
        db.upsert_ruleset(
            RuleScope::Project,
            Some(&project.id),
            Some(""),
            Some(md_path.to_str().unwrap()),
            None,
        )
        .unwrap();

        let exported = export_project(&db, &project.id, 1).unwrap();
        let value: serde_json::Value = serde_json::from_str(&exported).unwrap();

        assert_eq!(value["ruleset"]["mdContent"], "");
    }

    // ---- global ruleset (export_all / whole-store import) ----

    /// Seeds a `scope='global'` ruleset row + its `global.md` file under `app_support`, the way a
    /// real daemon boot does (`boot::ensure_global_ruleset` — not reachable from here, so this
    /// mimics it directly). Returns the seeded row's id so callers can assert whether a later
    /// reconcile kept it. The row's id is a fresh uuid (like boot's), so two DBs seeded this way
    /// get DIFFERENT global-row ids — exactly the situation whole-store restore has to reconcile.
    fn seed_global_ruleset(db: &Db, app_support: &Path, content: &str) -> String {
        let global_path = app_support.join("rules").join("global.md");
        std::fs::create_dir_all(global_path.parent().unwrap()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        db.conn()
            .execute(
                "INSERT INTO ruleset
                   (id, scope, project_id, md_path, md_hash, policy, created_at, updated_at)
                 VALUES (?1, 'global', NULL, ?2, '', '{}', 0, 0)",
                rusqlite::params![id, global_path.to_str().unwrap()],
            )
            .unwrap();
        db.upsert_ruleset(RuleScope::Global, None, Some(content), None, None)
            .unwrap();
        id
    }

    #[test]
    fn export_all_includes_a_non_null_global_ruleset_and_it_round_trips_under_app_support() {
        let db = Db::open_in_memory().unwrap();
        let app_support = tempfile::tempdir().unwrap();

        // `Db::open_in_memory()` alone never ensures the global ruleset row — that's
        // `boot::ensure_global_ruleset`, run at real daemon boot, and `boot` isn't reachable from
        // here (crate-private, `boot.rs`-only). Seed the row directly so this test can exercise
        // `export_all`'s non-null `globalRuleset` branch without depending on `boot`.
        seed_global_ruleset(&db, app_support.path(), "# global rules\n");

        let exported = export_all(&db, 1).unwrap();
        let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(value["globalRuleset"]["mdContent"], "# global rules\n");

        // Import into a TRULY empty store (no seeded global row): the global row inserts verbatim
        // (this is the spec §8 "import into an empty store" round-trip DoD path).
        let fresh = Db::open_in_memory().unwrap();
        let counts = import_bundle(&fresh, app_support.path(), &exported).unwrap();
        assert_eq!(counts.rulesets, 1);

        let imported = fresh.get_ruleset(RuleScope::Global, None).unwrap();
        assert!(Path::new(&imported.md_path).starts_with(app_support.path()));
        assert_eq!(
            std::fs::read_to_string(&imported.md_path).unwrap(),
            "# global rules\n"
        );
    }

    /// CRITICAL #2 regression: whole-store restore into a BOOTED daemon. A real daemon has already
    /// pre-seeded exactly one `scope='global'` row at boot, so a blind `INSERT` of the bundle's
    /// own global row would collide with the `ruleset_single_global` partial unique index →
    /// `Conflict` → the whole one-tx import rolls back, losing every project/task too. Import must
    /// RECONCILE the global row in place instead.
    #[test]
    fn import_into_a_boot_seeded_store_reconciles_the_global_ruleset() {
        // Staging store: its own seeded global row (+ content) and a full project, then export_all.
        let staging = Db::open_in_memory().unwrap();
        let staging_support = tempfile::tempdir().unwrap();
        seed_global_ruleset(&staging, staging_support.path(), "# staging global\n");
        build_fixture(&staging, staging_support.path(), "w1");
        let exported = export_all(&staging, 1).unwrap();

        // Target store: simulate a booted daemon — it ALREADY has its own seeded global row, with
        // a DIFFERENT id and different content.
        let target = Db::open_in_memory().unwrap();
        let target_support = tempfile::tempdir().unwrap();
        let seeded_global_id =
            seed_global_ruleset(&target, target_support.path(), "# target seed\n");

        // Must SUCCEED (not Conflict): projects + tasks + the reconciled global row all land.
        let counts = import_bundle(&target, target_support.path(), &exported).unwrap();
        assert_eq!(counts.projects, 1);
        assert_eq!(counts.tasks, 2);
        assert!(counts.rulesets >= 1);

        // Still EXACTLY one global row.
        let global_count: i64 = target
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM ruleset WHERE scope = 'global'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(global_count, 1, "must never end up with two global rows");

        // The seeded singleton is reconciled IN PLACE: its id is preserved (a boot impl detail,
        // not meaningful data), but its content now reflects the imported bundle.
        let imported_global = target.get_ruleset(RuleScope::Global, None).unwrap();
        assert_eq!(
            imported_global.id, seeded_global_id,
            "reconcile must keep the boot-seeded global row's id, not the bundle's"
        );
        assert!(Path::new(&imported_global.md_path).starts_with(target_support.path()));
        assert_eq!(
            std::fs::read_to_string(&imported_global.md_path).unwrap(),
            "# staging global\n",
            "the reconciled global row must carry the imported bundle's content"
        );
    }

    // ---- SECURITY: path-traversal on import (CRITICAL #1) ----

    #[test]
    fn import_rejects_a_ruleset_md_path_with_dotdot_traversal_and_writes_nothing_outside() {
        // A crafted bundle whose ruleset md_path lexically "starts_with" app_support but climbs
        // out via `..`. `Path::starts_with` is purely lexical, so the pre-fix code treated this as
        // "contained" and wrote `pwned` outside the tempdir. app_support is NESTED inside its own
        // `outer` tempdir so the `..` escape target lands INSIDE `outer` (cleaned on drop) — a
        // regression can never leak a file into the shared system temp dir across runs.
        let target = Db::open_in_memory().unwrap();
        let outer = tempfile::tempdir().unwrap();
        let app_support = outer.path().join("app-support");
        std::fs::create_dir_all(&app_support).unwrap();
        // `{app_support}/../traversal-escape.md` resolves to `{outer}/traversal-escape.md`.
        let escape_target = outer.path().join("traversal-escape-must-not-exist.md");
        let evil_md_path = app_support
            .join("..")
            .join("traversal-escape-must-not-exist.md");

        let project_id = "11111111-1111-1111-1111-111111111111";
        let json = serde_json::json!({
            "bundleFormat": 1, "exportedAt": 0,
            "project": {
                "id": project_id, "name": "P", "description": "",
                "status": "active", "workspaceIds": [],
                "createdAt": 0, "updatedAt": 0
            },
            "goals": [], "ideas": [], "insights": [], "tasks": [],
            "ruleset": {
                "rule": {
                    "id": "r1", "scope": "project", "projectId": project_id,
                    "mdPath": evil_md_path.to_str().unwrap(), "mdHash": "",
                    "policy": {"spendCapUsd": null, "approvalClasses": [], "pathAllowlist": []},
                    "createdAt": 0, "updatedAt": 0
                },
                "mdContent": "pwned"
            }
        })
        .to_string();

        let err = import_bundle(&target, &app_support, &json).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Validation(_)),
            "a `..` md_path must be rejected as Validation, got {err:?}"
        );
        assert!(
            !escape_target.exists(),
            "no file may be written outside app_support at {}",
            escape_target.display()
        );
        // Whole import rolled back — nothing landed.
        assert_eq!(table_counts(&target).iter().sum::<i64>(), 0);
    }

    #[test]
    fn import_rejects_a_ruleset_project_id_with_a_path_separator() {
        // A crafted bundle whose ruleset md_path is FOREIGN (forces the default-path repoint) and
        // whose project_id carries a traversal segment — `default_ruleset_md_path` would otherwise
        // interpolate it straight into `project-<id>.md` and escape. app_support is nested in an
        // `outer` tempdir (see the md_path traversal test) so a `project-../../evil-pid-escape.md`
        // escape would land INSIDE `outer`, never the shared system temp dir.
        let target = Db::open_in_memory().unwrap();
        let outer = tempfile::tempdir().unwrap();
        let app_support = outer.path().join("app-support");
        std::fs::create_dir_all(&app_support).unwrap();
        // `{app_support}/rules/project-../../evil-pid-escape.md` resolves to `{outer}/evil-pid-escape.md`.
        let escape_target = outer.path().join("evil-pid-escape.md");

        // project_id matches project.id (FK), so the bundle is structurally valid but malicious.
        let evil_pid = "../../evil-pid-escape";
        let json = serde_json::json!({
            "bundleFormat": 1, "exportedAt": 0,
            "project": {
                "id": evil_pid, "name": "P", "description": "",
                "status": "active", "workspaceIds": [],
                "createdAt": 0, "updatedAt": 0
            },
            "goals": [], "ideas": [], "insights": [], "tasks": [],
            "ruleset": {
                "rule": {
                    "id": "r1", "scope": "project", "projectId": evil_pid,
                    "mdPath": "/definitely/not/under/app-support/rules.md", "mdHash": "",
                    "policy": {"spendCapUsd": null, "approvalClasses": [], "pathAllowlist": []},
                    "createdAt": 0, "updatedAt": 0
                },
                "mdContent": "pwned"
            }
        })
        .to_string();

        let err = import_bundle(&target, &app_support, &json).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Validation(_)),
            "a project_id with a path separator must be rejected as Validation, got {err:?}"
        );
        assert!(
            !escape_target.exists(),
            "no file may be written outside app_support at {}",
            escape_target.display()
        );
        assert_eq!(table_counts(&target).iter().sum::<i64>(), 0);
    }
}
