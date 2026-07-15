//! bpa-orchd — Builder Pro AI orchestration daemon (library surface for integration tests).
//! Mirrors `bpa_sessiond`'s crate-root shape minus PTY concerns (spec §5).

pub use bpa_orchd_proto as protocol;

/// Export / import (spec §8, D7): `bundleFormat: 1` JSON bundles, field-verbatim import,
/// frame-cap guard. Builds directly on `persistence::Db`'s public CRUD/getter surface plus a
/// small set of `pub(crate)` raw-insert helpers `persistence` exposes just for this module's
/// import transaction.
pub mod export;
pub mod persistence;
pub mod socket_server;

/// MCP server/tool registry persistence (S-EXT spec §4 schema v3). `pub` (unlike `mod graph;`
/// below) — see `mcp`'s own module doc comment for why: no `bpa_orchd_proto` wire types exist for
/// MCP yet (T3), so this module's row/enum types need to stay independently nameable.
pub mod mcp;

/// Trust choke-point (S-EXT spec §6, D10, task T5): the single pre-dispatch gate every MCP
/// connect / tool-call passes through (`trust::authorize`). `pub` — `mcp::lifecycle`/
/// `mcp::invoke` (sibling top-level-adjacent modules, not nested under `mcp`) both call into it,
/// and a later task's `socket_server` dispatch will too (e.g. `TrustGrantConsent`).
pub mod trust;

/// Connector OAuth account layer (S-EXT spec §4 `account` table, §5/§7, D5, task T11): `account`
/// CRUD + the OAuth 2.1 authorization-code+PKCE flow driver (`oauth2` crate) + API-key accounts,
/// tokens in Keychain, only refs in the DB. `pub` — same reasoning as `mcp` above (this module's
/// own `AccountRow`/`NewAccount`/`AccountAuthKind` types, plus `ConnectorsState`, need to stay
/// independently nameable for a later task's `socket_server` dispatch wiring, T13a).
pub mod connectors;

/// Skills registry persistence (S-EXT spec §4 `skill` table, §8, D11, Q14, task T17): SKILL.md
/// CRUD + frontmatter parsing + files-as-truth status. `pub` — same reasoning as `mcp`/
/// `connectors` above (this module's own `SkillRow`/`SkillView`/`NewSkill`/`SkillScope` types need
/// to stay independently nameable for `socket_server`'s `Skill*` dispatch arms, this same task).
pub mod skills;

/// Research-run persistence (S-IDEA spec §4/§6 schema v4, D11, task T2): the idea↔invocation↔
/// artifact provenance link a research run leaves behind + the boot-reconcile of interrupted
/// runs. `pub` — same reasoning as `mcp`/`connectors`/`skills` above (this module's own
/// `ResearchRunRow`/`NewResearchRun`/`ResearchStatus` types need to stay independently nameable
/// for a later task's `socket_server` dispatch wiring, T5, and the run driver, T4).
pub mod research;

mod boot;
/// Knowledge-graph node/edge persistence (S4 spec §4 schema v2, §5 persistence + invariants).
/// Crate-private — `persistence::Db`'s CRUD/getter surface (`conn()`, `ensure_project_active`,
/// `now_ms`, `is_constraint_violation`) is the seam this module builds on, mirroring how
/// `ruleset_files` builds on `persistence::Db`'s ruleset methods.
mod graph;
/// RuleSet markdown FILE layer (spec §7, D4): atomic writes + read-fresh state classification.
/// Crate-private — `persistence::Db`'s ruleset methods are the public surface that builds on it.
mod ruleset_files;
/// Test-support hook so integration tests can assert their `$HOME` isolation actually redirects
/// the daemon's on-disk DB/rules path (mirrors `bpa_sessiond::app_support_dir_for_test`). See
/// [`boot::app_support_dir_for_test`].
pub use boot::app_support_dir_for_test;
/// Testable daemon boot core (spec §5): bind, open the DB, ensure the global ruleset, run
/// `serve` until shutdown, then drain. `main.rs` is a thin process-concerns wrapper over this.
pub use boot::run;
