//! Skills registry persistence (S-EXT spec §4 schema v3 `skill` table, §8, D11, Q14, task T17).
//! Sibling module to [`crate::mcp`]/[`crate::connectors`] — mirrors their file-layout shape
//! (`mod.rs` holds the row/enum/input types + the `bpa_orchd_proto` conversions, [`registry`]
//! holds the `impl persistence::Db` CRUD, mirroring `mcp::registry`'s "row-struct + enum⇄TEXT +
//! CRUD-on-`Db`" pattern byte-for-byte). The `skill` table itself was already created by T2's
//! `persistence::migrate_v3` (spec §4 DDL, code-truth since T2) — this module only ADDS
//! persistence CRUD + SKILL.md file handling on top, no migration/schema work here.
//!
//! **Plumbing only (D11): there is no runtime consumer of this registry yet** — the agent org
//! that would actually LOAD and execute a registered skill ships in S6b. This module (and the
//! `SkillsTab` UI built on top of it in the same task) is honestly a CRUD registry: it validates,
//! stores, and surfaces SKILL.md files, nothing else.
//!
//! **Files-as-truth (D11, mirrors RuleSet's D4 of S3)**: the DB stores `md_path` + a sha256
//! `md_hash` computed from the file's bytes at `add_skill` time — the SKILL.md file itself
//! remains the source of truth for its content. [`registry::compute_file_state`] re-hashes the
//! file at read time (`list_skills`) and reports honestly when it has drifted (`Modified`) or
//! disappeared (`Missing`) since it was registered, rather than silently serving a stale DB
//! snapshot.
//!
//! **SKILL.md format (Q14)**: adopts the Claude Code `SKILL.md` convention — a markdown file
//! whose top begins with a `---`-delimited YAML frontmatter block carrying (at minimum) `name`/
//! `description` keys, followed by the skill's actual body content. [`registry::parse_frontmatter`]
//! is a MINIMAL parser for that block (`key: value` scalar lines only — no nested maps, lists, or
//! multiline strings), used by `add_skill` to fill in `name`/`description` when the caller omits
//! them.

pub mod registry;

/// `skill.scope` (spec §4 CHECK: `scope IN ('global','project')`). A separate type from
/// `bpa_orchd_proto::SkillScope` (the wire enum) for the same reason `mcp::McpScope`/
/// `connectors::AccountAuthKind` predate (and are independent of) any wire enum — this crate's
/// row/enum types are the schema-shaped source of truth [`registry`]'s CRUD operates on; the wire
/// crate has its own copy for the (possibly narrower, possibly differently-cased) DTO it
/// serializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Global,
    Project,
}

/// Files-as-truth read-time classification (mirrors `bpa_orchd_proto::SkillFileState`'s three
/// variants exactly — see [`registry::compute_file_state`] for how this is computed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFileState {
    Present,
    Modified,
    Missing,
}

/// Full `skill` row, decoded (spec §4 columns; `scope` TEXT column decoded into [`SkillScope`] —
/// mirrors `mcp::McpServerRow`'s decode shape). Doubles as both the raw-row AND the return type of
/// `Db::add_skill`/`Db::delete_skill`'s lookup — there is no separate wire DTO at this layer (that
/// is `bpa_orchd_proto::Skill`, built from [`SkillView`] by [`SkillRow::into_view`] below).
#[derive(Debug, Clone, PartialEq)]
pub struct SkillRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub md_path: String,
    pub md_hash: String,
    pub scope: SkillScope,
    pub project_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl SkillRow {
    /// Wraps this row with a FRESH [`SkillFileState`] computed by re-reading its `md_path` right
    /// now ([`registry::compute_file_state`]) — the single conversion both `Db::add_skill`'s
    /// caller (`socket_server`'s `SkillAdd` dispatch arm) and `Db::list_skills` use to attach the
    /// files-as-truth status, so the two paths can never compute it two different ways.
    pub fn into_view(self) -> SkillView {
        let file_state = registry::compute_file_state(&self.md_path, &self.md_hash);
        SkillView {
            skill: self,
            file_state,
        }
    }
}

/// [`SkillRow`] plus its files-as-truth read-time status (spec §8 brief: "`SkillView` = the row
/// PLUS a files-as-truth status"). Mirrors `bpa_orchd_proto::RuleSetView`'s "DB row + fresh file
/// read" shape, minus the file CONTENT itself — the skills UI only needs a badge
/// («modified»/«file missing»), never the raw markdown, so [`SkillFileState`] alone (no
/// `Option<String>` content field) is the honest, minimal shape here.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillView {
    pub skill: SkillRow,
    pub file_state: SkillFileState,
}

/// Input to `Db::add_skill` (task T17 brief). `name`/`description: None` ⇒ parsed from the
/// SKILL.md frontmatter at `md_path` (Q14) by `registry::add_skill`; `id`/`md_hash`/
/// `created_at`/`updated_at` are assigned by the insert itself (uuid v4 / a fresh sha256 of the
/// file's bytes / `now_ms()`) — never supplied by the caller, mirrors `mcp::NewMcpServer`'s shape.
#[derive(Debug, Clone)]
pub struct NewSkill {
    pub name: Option<String>,
    pub description: Option<String>,
    pub md_path: String,
    pub scope: SkillScope,
    pub project_id: Option<String>,
}

impl From<SkillScope> for bpa_orchd_proto::SkillScope {
    fn from(s: SkillScope) -> Self {
        match s {
            SkillScope::Global => bpa_orchd_proto::SkillScope::Global,
            SkillScope::Project => bpa_orchd_proto::SkillScope::Project,
        }
    }
}

impl From<SkillFileState> for bpa_orchd_proto::SkillFileState {
    fn from(s: SkillFileState) -> Self {
        match s {
            SkillFileState::Present => bpa_orchd_proto::SkillFileState::Present,
            SkillFileState::Modified => bpa_orchd_proto::SkillFileState::Modified,
            SkillFileState::Missing => bpa_orchd_proto::SkillFileState::Missing,
        }
    }
}

/// [`SkillView`] -> the wire `Skill` entity (task T17, spec §5 dispatch): flattens the nested
/// `skill`/`file_state` shape into `bpa_orchd_proto::Skill`'s flat field set — mirrors
/// `connectors::AccountRow`'s own `From` impl shape (that module's doc comment on why this
/// conversion lives here, next to the row types, rather than in `socket_server`).
impl From<SkillView> for bpa_orchd_proto::Skill {
    fn from(v: SkillView) -> Self {
        bpa_orchd_proto::Skill {
            id: v.skill.id,
            name: v.skill.name,
            description: v.skill.description,
            md_path: v.skill.md_path,
            md_hash: v.skill.md_hash,
            scope: v.skill.scope.into(),
            project_id: v.skill.project_id,
            file_state: v.file_state.into(),
            created_at: v.skill.created_at,
            updated_at: v.skill.updated_at,
        }
    }
}
