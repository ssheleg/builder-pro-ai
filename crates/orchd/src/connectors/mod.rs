//! Connector OAuth account layer (S-EXT spec §4 `account` table, §5 Connector verbs, §7, D5, task
//! T11). Sibling module to [`crate::mcp`] — mirrors its file-layout shape (`mod.rs` holds the
//! row/enum/input types shared by the CRUD + flow logic, [`accounts`] holds the `impl
//! persistence::Db` account CRUD PLUS the OAuth 2.1 authorization-code+PKCE flow driver, mirroring
//! `mcp::registry`'s "row-struct + enum⇄TEXT pattern" byte-for-byte). The `account` table itself
//! was already created by T2's `persistence::migrate_v3` (spec §4 DDL, code-truth since T2) — this
//! module only ADDS persistence CRUD + OAuth driver logic on top, no migration/schema work here.
//!
//! Deliberately does NOT reuse `bpa_orchd_proto::Account` (T10's wire DTO) as the row type:
//! that entity intentionally OMITS `secret_ref`/`refresh_ref` (T10's design note: "no UI surface
//! reads a Keychain key name"), but this module's OWN read type — [`AccountRow`], playing the same
//! role `mcp::McpServerRow` plays for `mcp_server` — needs both, since [`accounts::token_for`] must
//! resolve the Keychain entry they point at. `bpa_orchd_proto::OAuthChallenge` IS reused directly
//! for [`accounts::ConnectorsState::begin_oauth`]'s return type: that struct is a plain 2-field
//! `{authorize_url, state}` shape with no secret-bearing fields, so there is nothing to gain by
//! duplicating it.

pub mod accounts;

/// `account.auth_kind` (spec §4: `'oauth' | 'apikey'`). A separate type from
/// `bpa_orchd_proto::AccountAuthKind` (T10's wire enum) for the same reason `mcp::McpAuthKind`
/// predates (and is independent of) any wire enum — this crate's row/enum types are the
/// schema-shaped source of truth [`accounts`]'s CRUD operates on; the wire crate has its own copy
/// for the (narrower) DTO it serializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountAuthKind {
    Oauth,
    Apikey,
}

/// Full `account` row, decoded (spec §4 columns; `scopes_json` TEXT column decoded into
/// `scopes`, `auth_kind` TEXT column decoded into [`AccountAuthKind`] — mirrors
/// `mcp::McpServerRow`'s decode shape). `secret_ref` is the Keychain "account" key string (spec
/// §4 comment: "Keychain account key (token/apikey lives there)") — NEVER the secret value
/// itself; same non-secret shape as `mcp::McpServerRow::secret_ref`.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub auth_kind: AccountAuthKind,
    pub secret_ref: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub refresh_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Input to `persistence::Db::insert_account` (task T11). Deliberately UNLIKE `mcp::NewMcpServer`
/// (which leaves `id` for the insert to assign internally via `Uuid::new_v4()`): here the CALLER
/// supplies `id`, because `account.secret_ref` is `NOT NULL` (spec §4) — the Keychain write
/// (`bpa_secrets::account_ref(id, kind)`, keyed by this same `id`) must happen BEFORE the DB
/// insert, so the id has to exist first. `mcp_server.secret_ref` sidesteps this (it's nullable, so
/// `add_mcp_server` can insert row-first-secret-later via the separate `McpSetServerBearer` /
/// `set_mcp_server_secret_ref` two-step) — `account` has no such nullable escape hatch. See
/// [`accounts::ConnectorsState::complete_oauth`]/[`accounts::ConnectorsState::add_apikey`] for the
/// id-generation + Keychain-write-then-insert ordering this shape exists for.
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub auth_kind: AccountAuthKind,
    pub secret_ref: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub refresh_ref: Option<String>,
}
