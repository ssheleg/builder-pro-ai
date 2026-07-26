//! `account` CRUD (spec §4) + the OAuth 2.1 authorization-code+PKCE flow driver (spec §5/§7, D5,
//! task T11). Builds directly on `persistence::Db`'s `conn()`/`now_ms`/`OrchdPersistError` seam —
//! same pattern `mcp::registry` established for `mcp_server`/`mcp_tool` (row-struct + enum⇄TEXT
//! mapping); see this crate's `mcp::registry` module doc comment for the shared rationale.
//!
//! **Provider-config seam (documented per the task brief's explicit choice point).** `oauth_config`
//! (client_id/secret/auth_url/token_url) is NOT a parameter of `begin_oauth` — the wire verb
//! `ConnectorBeginOAuth{provider,label,scopes?,server_id?}` (spec §5) carries no such fields, and
//! threading raw IdP credentials through every call site (and eventually the wire protocol) would
//! leak them far beyond where they need to live. Instead [`ConnectorsState`] holds a small
//! **provider registry** (`register_oauth_provider(provider, OAuthProviderConfig)`), keyed by the
//! same `provider` string the wire verbs already carry; `begin_oauth`/`complete_oauth`/`token_for`
//! look the config up internally. v1 has no seeded providers — a later task (T13a, daemon boot, or
//! a config-file-backed registry) calls `register_oauth_provider` for real IdPs (prowl.chat, X,
//! LinkedIn, ...); this task's own tests call it directly to drive the flow hermetically.
//!
//! **Keychain-write-then-DB-insert ordering (see [`super::NewAccount`]'s doc comment).**
//! `complete_oauth`/`add_apikey` generate the account `id` themselves, write the Keychain entry
//! under that id FIRST, then call `Db::insert_account` with the id supplied — `account.secret_ref`
//! is `NOT NULL` (spec §4), so there is no row-first-secret-later two-step available the way
//! `mcp_server`'s nullable `secret_ref` gets.
//!
//! **DB-lock-across-network-await discipline (mirrors `mcp::invoke::call_tool`/
//! `mcp::lifecycle::connect`, S-EXT task T6's review-fix precedent — see those modules' doc
//! comments for the original incident: holding the single daemon-wide `Db` mutex for the duration
//! of a network round-trip stalls every other orchd connection).** `complete_oauth`/`token_for` are
//! the two methods here that mix a network round-trip (the OAuth token endpoint) with DB access;
//! both take the SHARED `Arc<tokio::sync::Mutex<Db>>` (the exact type `socket_server::ServerDeps.db`
//! holds) and lock it only in short-lived phases strictly BEFORE or AFTER the network `.await`,
//! never across it. `add_apikey`/`insert_account`/`get_account`/`list_accounts`/`delete_account`/
//! `update_account_tokens` never touch the network, so they take a plain `&Db`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use bpa_orchd_proto::OAuthChallenge;

use crate::persistence::{is_constraint_violation, now_ms, Db, OrchdPersistError};

use super::{AccountAuthKind, AccountRow, NewAccount};

/// A fully-configured `BasicClient` — `auth_uri` AND `token_uri` both set (spec §5:
/// `authorize_url`/`exchange_code`/`exchange_refresh_token` all need `token_uri`; `authorize_url`
/// additionally needs `auth_uri`). `oauth2` 5.0's typestate only tracks the URL-shaped endpoints
/// (`HasAuthUrl`.../`HasTokenUrl`) — `client_secret`/`redirect_uri` are plain optional setters that
/// do NOT change the type (verified against the vendored 5.0.0 source, `src/client.rs`), so ONE
/// alias covers every use site in this file (`begin_oauth`'s `authorize_url()`, `complete_oauth`'s
/// `exchange_code()`, `token_for`'s `exchange_refresh_token()`) regardless of whether a redirect
/// URI was supplied.
type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// An OAuth 2.1 IdP's endpoints + this app's registered client credentials for one `provider`
/// string (spec §4 `account.provider`, e.g. `"prowl"`). See this module's doc comment for why this
/// is a registry entry rather than a `begin_oauth` parameter.
#[derive(Clone)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    /// Public clients (PKCE-only, e.g. a native/desktop app registration) have none.
    pub client_secret: Option<String>,
    pub auth_url: String,
    pub token_url: String,
    /// Provider-level default OAuth scopes (spec D7's `oauth_providers.json` `default_scopes?`
    /// field). Applied by [`ConnectorsState::begin_oauth`] ONLY when the caller passes an empty
    /// `scopes` slice (the UI's optional scopes field left blank) — an explicit caller-supplied
    /// scope list always wins, and this fallback never widens a scope set the caller narrowed.
    /// Empty when the config file omits `default_scopes`. Public OAuth-registration values, not a
    /// secret.
    pub default_scopes: Vec<String>,
}

// Hand-written `Debug` redacts `client_secret` (D4, no-secret-in-logs): the derived impl would
// print a live client secret the first time this config is `{:?}`-formatted / `#[instrument]`-ed.
// `client_id`/`auth_url`/`token_url` are public OAuth-registration values, safe to show.
impl std::fmt::Debug for OAuthProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthProviderConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("default_scopes", &self.default_scopes)
            .finish()
    }
}

/// The resolved bearer credential [`ConnectorsState::token_for`] hands to a caller (T12's
/// connector-invoke / MCP-server-OAuth consumer) — never persisted, never logged; lives only for
/// the duration of the one call that needed it.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountToken {
    pub bearer: String,
}

// Hand-written `Debug` redacts the live bearer (D4, no-secret-in-logs): this value is handed to
// T12's connector-invoke consumer, so a downstream `tracing::debug!(?token)` / `#[instrument]`
// with the derived impl would leak a working credential to the logs.
impl std::fmt::Debug for AccountToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountToken")
            .field("bearer", &"[REDACTED]")
            .finish()
    }
}

/// Errors from the connector OAuth/apikey flow driver AND (task T12) the direct-API
/// [`super::adapter::ConnectorAdapter`] invoke path — one shared error type for the whole
/// `connectors` module (spec §7's `ConnectorAdapter::invoke` signature names `ConnectorError` as
/// its error type directly, so T12 extends this enum rather than inventing a parallel one).
/// `Display`/`Debug` never carry secret bytes: `TokenExchange`/`Http`/`Request` wrap the
/// oauth2/reqwest library's OWN error text (HTTP/transport/parse failure descriptions, never
/// response bodies containing a token/bearer — a *successful* call never routes through those
/// variants, and `UpstreamStatus` carries only the numeric status code, never the response body),
/// and every other variant is a plain non-secret string.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("unknown OAuth provider: {0}")]
    UnknownProvider(String),
    #[error("unknown or expired OAuth state")]
    UnknownState,
    #[error("invalid OAuth provider config: {0}")]
    InvalidConfig(String),
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("http client build failed: {0}")]
    Http(String),
    #[error("stored secret is not valid utf-8")]
    SecretNotUtf8,
    /// (T12) No [`super::adapter::ConnectorAdapter`] is registered for this `account.provider`
    /// string — distinct from `UnknownProvider` (which is specifically about the OAuth
    /// IdP-config registry `begin_oauth`/`token_for` consult; an `apikey` account, which never
    /// touches that registry at all, can still fail here).
    #[error("no connector adapter for provider: {0}")]
    NoAdapter(String),
    /// (T12) `ConnectorAdapter::invoke` was called with an `op` the adapter doesn't recognize.
    #[error("unknown connector operation: {0}")]
    UnknownOp(String),
    /// (T12) `args` was missing a required field or had the wrong shape for `op`.
    #[error("invalid connector op arguments: {0}")]
    InvalidArgs(String),
    /// (T12) The adapter's outbound HTTP request itself failed (DNS/connect/parse) — a
    /// non-timeout transport failure. Wraps `reqwest::Error`'s own `Display`, never the request
    /// bearer or body.
    #[error("connector request failed: {0}")]
    Request(String),
    /// (T12) The adapter's outbound HTTP request exceeded its bounded timeout.
    #[error("connector request timed out")]
    Timeout,
    /// (T12) The remote API answered with a non-2xx status. Carries ONLY the numeric status —
    /// never the response body (spec §6: never log tool/op result content).
    #[error("connector upstream returned status {0}")]
    UpstreamStatus(u16),
    /// (B1) The remote API's response body exceeded the bounded read cap. A connector result is
    /// untrusted-class (`is_untrusted=1`): an arbitrarily large / chunked (no Content-Length) body
    /// must never be buffered wholesale, or a single hostile response OOM-crashes all of orchd.
    /// Carries only the cap (bytes) — never any body content.
    #[error("connector response body exceeded {0} bytes")]
    OversizedBody(usize),
    #[error(transparent)]
    Persist(#[from] OrchdPersistError),
    #[error(transparent)]
    Secret(#[from] bpa_secrets::SecretError),
}

/// An in-flight `begin_oauth` request, keyed by its CSRF `state` (spec §5's
/// `OAuthChallenge.state`) until the matching `complete_oauth` — or forever, if the owner never
/// completes it (v1 has no expiry sweep; a stale pending entry is inert, not a resource leak
/// worth guarding against yet — same "no TTL" honesty other in-memory maps in this codebase start
/// with before a later task adds one, if ever needed).
struct PendingOAuth {
    provider: String,
    label: String,
    scopes: Vec<String>,
    redirect: String,
    pkce_verifier: PkceCodeVerifier,
}

/// Shared connector state: the OAuth provider registry + the in-flight `begin_oauth` pending map.
/// Both are process-local, in-memory `std::sync::Mutex`-guarded maps (never held across an
/// `.await` — everything in this file that touches the network drops its lock first), distinct
/// from the durable `account` table `persistence::Db` owns.
#[derive(Default)]
pub struct ConnectorsState {
    providers: StdMutex<HashMap<String, OAuthProviderConfig>>,
    pending: StdMutex<HashMap<String, PendingOAuth>>,
}

impl ConnectorsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers (or overwrites) provider `provider`'s OAuth endpoint/credential config. See this
    /// module's doc comment for why this seam exists instead of a `begin_oauth` parameter.
    pub fn register_oauth_provider(&self, provider: &str, config: OAuthProviderConfig) {
        self.providers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(provider.to_string(), config);
    }

    /// The NAMES of every registered OAuth provider, sorted for a stable/deterministic order
    /// (`ConnectorListProviders`, spec D7, O-5). Names ONLY — a provider's
    /// `client_id`/`client_secret`/endpoint URLs never leave [`OAuthProviderConfig`] through this
    /// accessor (the wire response is names-only, spec D7). An empty registry ⇒ an empty `Vec`,
    /// never an error — the honest v1 default before any `oauth_providers.json` exists.
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .providers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// Test-support introspection: whether a pending `begin_oauth` entry is stored under `state`.
    /// `#[doc(hidden)]`, not part of the stable API — mirrors `persistence::Db::conn()`'s own
    /// "test-support seam" precedent (this crate's tests have no other way to assert in-flight
    /// pending-map state without leaking `PendingOAuth`, which deliberately stays private since it
    /// carries a live `PkceCodeVerifier`).
    #[doc(hidden)]
    pub fn has_pending(&self, state: &str) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(state)
    }

    /// `ConnectorBeginOAuth` (spec §5): builds the registered provider's client, generates a fresh
    /// PKCE challenge/verifier pair + CSRF state, returns the browser-ready authorize URL, and
    /// stashes the verifier + in-flight metadata in the pending map keyed by that state for the
    /// matching [`Self::complete_oauth`]. No network I/O (the authorize URL is constructed
    /// locally; the owner's browser is what actually hits it).
    pub fn begin_oauth(
        &self,
        provider: &str,
        label: &str,
        scopes: &[String],
        redirect: &str,
    ) -> Result<OAuthChallenge, ConnectorError> {
        let config = self.provider_config(provider)?;
        let client = build_client(&config, Some(redirect))?;

        // Provider `default_scopes` fill in ONLY for an empty caller list (spec D7): an explicit
        // caller-supplied scope set always wins and is never widened by the fallback. The
        // effective set is what's both requested on `/authorize` AND recorded on the resulting
        // `account` row (via the pending entry), so the account honestly reflects the scopes it
        // was actually granted under.
        let effective_scopes: &[String] = if scopes.is_empty() {
            &config.default_scopes
        } else {
            scopes
        };

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);
        for scope in effective_scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }
        let (authorize_url, csrf_token) = request.url();
        let state = csrf_token.secret().clone();

        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                state.clone(),
                PendingOAuth {
                    provider: provider.to_string(),
                    label: label.to_string(),
                    scopes: effective_scopes.to_vec(),
                    redirect: redirect.to_string(),
                    pkce_verifier,
                },
            );

        Ok(OAuthChallenge {
            authorize_url: authorize_url.to_string(),
            state,
        })
    }

    /// `ConnectorCompleteOAuth` (spec §5): looks up the pending entry by `state` (removing it —
    /// one-shot, like the authorization code itself), exchanges `code` for tokens via the
    /// SSRF-guarded HTTP client (D5 locked DoD), stores the access token (and refresh token, if
    /// issued) in Keychain, then inserts the `account` row. The token bytes NEVER touch `db` or
    /// any log — only the deterministic Keychain ref STRING (`bpa_secrets::account_ref(id,
    /// "token"|"refresh").account`) is persisted.
    pub async fn complete_oauth(
        &self,
        db: &Arc<TokioMutex<Db>>,
        state: &str,
        code: &str,
    ) -> Result<AccountRow, ConnectorError> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(state)
            .ok_or(ConnectorError::UnknownState)?;
        let PendingOAuth {
            provider,
            label,
            scopes,
            redirect,
            pkce_verifier,
        } = pending;

        let config = self.provider_config(&provider)?;
        let client = build_client(&config, Some(&redirect))?;
        let http = ssrf_guarded_http_client()?;

        // ---- network phase (no DB lock held) ----
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http)
            .await
            .map_err(|e| ConnectorError::TokenExchange(e.to_string()))?;

        let account_id = Uuid::new_v4().to_string();
        bpa_secrets::set(
            &bpa_secrets::account_ref(&account_id, "token"),
            token.access_token().secret().as_bytes(),
        )?;
        let refresh_ref = match token.refresh_token() {
            Some(rt) => {
                bpa_secrets::set(
                    &bpa_secrets::account_ref(&account_id, "refresh"),
                    rt.secret().as_bytes(),
                )?;
                Some(bpa_secrets::account_ref(&account_id, "refresh").account)
            }
            None => None,
        };
        let expires_at = token.expires_in().map(|d| expires_at_from(now_ms(), d));

        let new_account = NewAccount {
            id: account_id.clone(),
            provider,
            label,
            auth_kind: AccountAuthKind::Oauth,
            secret_ref: bpa_secrets::account_ref(&account_id, "token").account,
            scopes,
            expires_at,
            refresh_ref,
        };

        // ---- DB phase (lock held only for the insert itself) ----
        let guard = db.lock().await;
        let row = guard.insert_account(new_account)?;
        Ok(row)
    }

    /// `ConnectorAddApiKey` (spec §5): stores `api_key` in Keychain under a freshly-generated
    /// account id, then inserts the `account` row (`auth_kind = 'apikey'`). No network I/O — plain
    /// `&Db`, no `Arc<Mutex<_>>` needed.
    pub fn add_apikey(
        &self,
        db: &Db,
        provider: &str,
        label: &str,
        api_key: &str,
    ) -> Result<AccountRow, ConnectorError> {
        let account_id = Uuid::new_v4().to_string();
        bpa_secrets::set(
            &bpa_secrets::account_ref(&account_id, "apikey"),
            api_key.as_bytes(),
        )?;
        let new_account = NewAccount {
            id: account_id.clone(),
            provider: provider.to_string(),
            label: label.to_string(),
            auth_kind: AccountAuthKind::Apikey,
            secret_ref: bpa_secrets::account_ref(&account_id, "apikey").account,
            scopes: Vec::new(),
            expires_at: None,
            refresh_ref: None,
        };
        Ok(db.insert_account(new_account)?)
    }

    /// Resolves the current bearer credential for `account_id` (T12's connector-invoke /
    /// MCP-server-OAuth consumer seam): an apikey account's Keychain value verbatim; an oauth
    /// account's cached access token UNLESS it's expired AND a refresh token is on file, in which
    /// case it's refreshed first (Keychain + `account.expires_at`/`refresh_ref` updated) before
    /// returning the new bearer. An expired token with NO refresh ref on file is returned as-is
    /// (honest degradation — the caller's downstream call will fail with the IdP's own 401 rather
    /// than this layer inventing a different error for a case it cannot fix).
    pub async fn token_for(
        &self,
        db: &Arc<TokioMutex<Db>>,
        account_id: &str,
    ) -> Result<AccountToken, ConnectorError> {
        let row = { db.lock().await.get_account(account_id)? };

        match row.auth_kind {
            AccountAuthKind::Apikey => {
                let bytes = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "apikey"))?;
                Ok(AccountToken {
                    bearer: bytes_to_string(bytes)?,
                })
            }
            AccountAuthKind::Oauth => {
                let expired = row.expires_at.map(|exp| exp <= now_ms()).unwrap_or(false);
                if expired && row.refresh_ref.is_some() {
                    self.refresh_oauth_token(db, &row).await
                } else {
                    let bytes = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "token"))?;
                    Ok(AccountToken {
                        bearer: bytes_to_string(bytes)?,
                    })
                }
            }
        }
    }

    async fn refresh_oauth_token(
        &self,
        db: &Arc<TokioMutex<Db>>,
        row: &AccountRow,
    ) -> Result<AccountToken, ConnectorError> {
        let config = self.provider_config(&row.provider)?;
        // No redirect_uri: not part of the refresh-token grant (RFC 6749 §6) and never stored
        // per-account — only `begin_oauth`/`complete_oauth` need it, and only for THAT flow.
        let client = build_client(&config, None)?;
        let http = ssrf_guarded_http_client()?;
        let refresh_bytes = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "refresh"))?;
        let refresh_token_str = bytes_to_string(refresh_bytes)?;

        // ---- network phase (no DB lock held) ----
        let token = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token_str))
            .request_async(&http)
            .await
            .map_err(|e| ConnectorError::TokenExchange(e.to_string()))?;

        let new_access = token.access_token().secret().clone();
        bpa_secrets::set(
            &bpa_secrets::account_ref(&row.id, "token"),
            new_access.as_bytes(),
        )?;
        let mut has_refresh = row.refresh_ref.is_some();
        if let Some(new_refresh) = token.refresh_token() {
            bpa_secrets::set(
                &bpa_secrets::account_ref(&row.id, "refresh"),
                new_refresh.secret().as_bytes(),
            )?;
            has_refresh = true;
        }
        let new_expires_at = token.expires_in().map(|d| expires_at_from(now_ms(), d));

        // ---- DB phase (lock held only for the update itself) ----
        {
            let guard = db.lock().await;
            guard.update_account_tokens(&row.id, new_expires_at, has_refresh)?;
        }

        Ok(AccountToken { bearer: new_access })
    }

    fn provider_config(&self, provider: &str) -> Result<OAuthProviderConfig, ConnectorError> {
        self.providers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(provider)
            .cloned()
            .ok_or_else(|| ConnectorError::UnknownProvider(provider.to_string()))
    }
}

fn duration_to_ms(d: Duration) -> i64 {
    d.as_millis().min(i64::MAX as u128) as i64
}

/// Absolute expiry (epoch ms) = `now + ttl`, saturating instead of overflowing i64 (B3).
/// `duration_to_ms` already clamps a huge `expires_in` to `i64::MAX`; `now + i64::MAX` would then
/// wrap to a NEGATIVE `expires_at` (so the token reads as permanently expired) or debug-panic.
fn expires_at_from(now: i64, d: Duration) -> i64 {
    now.saturating_add(duration_to_ms(d))
}

fn bytes_to_string(bytes: Vec<u8>) -> Result<String, ConnectorError> {
    String::from_utf8(bytes).map_err(|_| ConnectorError::SecretNotUtf8)
}

/// Builds a [`ConfiguredClient`] from `config`, optionally with a redirect URI (`begin_oauth`/
/// `complete_oauth` need the SAME redirect echoed on both the initial `/authorize` request and the
/// code exchange, per RFC 6749 §4.1.3; the refresh-token grant needs none — see
/// [`ConnectorsState::refresh_oauth_token`]).
fn build_client(
    config: &OAuthProviderConfig,
    redirect: Option<&str>,
) -> Result<ConfiguredClient, ConnectorError> {
    let auth_url = AuthUrl::new(config.auth_url.clone())
        .map_err(|e| ConnectorError::InvalidConfig(format!("auth_url: {e}")))?;
    let token_url = TokenUrl::new(config.token_url.clone())
        .map_err(|e| ConnectorError::InvalidConfig(format!("token_url: {e}")))?;

    let mut client = BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url);
    if let Some(secret) = &config.client_secret {
        client = client.set_client_secret(ClientSecret::new(secret.clone()));
    }
    if let Some(redirect) = redirect {
        let redirect_url = RedirectUrl::new(redirect.to_string())
            .map_err(|e| ConnectorError::InvalidConfig(format!("redirect_uri: {e}")))?;
        client = client.set_redirect_uri(redirect_url);
    }
    Ok(client)
}

/// SSRF guard (D5 locked DoD): the token-exchange/refresh HTTP client MUST disable redirect
/// following (oauth2-rs's own guidance — a redirect could smuggle the request, including the
/// `Authorization`/client-secret header for confidential clients, to an attacker-controlled host).
/// Uses `oauth2`'s OWN re-exported `reqwest` (`oauth2::reqwest`), NOT this workspace's top-level
/// `reqwest` crate — see the `oauth2` dependency's Cargo.toml comment: `oauth2` 5.0.0 pins
/// `reqwest = "0.12"`, a full major version behind this workspace's `reqwest = "0.13"` pin, so
/// Cargo resolves two separate crate instances and `AsyncHttpClient` is only implemented for
/// oauth2's own copy.
fn ssrf_guarded_http_client() -> Result<oauth2::reqwest::Client, ConnectorError> {
    ssrf_guarded_http_client_with_timeout(OAUTH_HTTP_TIMEOUT)
}

/// The token-exchange/refresh HTTP client's request timeout (BL-91, spec D5). Without it a
/// `ConnectorCompleteOAuth`/token-refresh against an unresponsive IdP hangs the connection's
/// dispatch task indefinitely (the `GenericRestAdapter` already sets its own 30s timeout, for
/// comparison). 30s matches that adapter.
const OAUTH_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Split from [`ssrf_guarded_http_client`] with an explicit `timeout` so the timeout behavior is
/// unit-testable with a short bound against a silent peer, without waiting the production 30s.
fn ssrf_guarded_http_client_with_timeout(
    timeout: std::time::Duration,
) -> Result<oauth2::reqwest::Client, ConnectorError> {
    oauth2::reqwest::ClientBuilder::new()
        .redirect(oauth2::reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|e| ConnectorError::Http(e.to_string()))
}

// ================================================================================
// ---- `account` persistence (spec §4): enum⇄TEXT + scopes JSON, row decode, Db CRUD ----
// ================================================================================

fn encode_auth_kind(k: &AccountAuthKind) -> &'static str {
    match k {
        AccountAuthKind::Oauth => "oauth",
        AccountAuthKind::Apikey => "apikey",
    }
}

fn decode_auth_kind(s: &str) -> Result<AccountAuthKind, OrchdPersistError> {
    match s {
        "oauth" => Ok(AccountAuthKind::Oauth),
        "apikey" => Ok(AccountAuthKind::Apikey),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt account.auth_kind value: {other}"
        ))),
    }
}

fn encode_scopes(scopes: &[String]) -> Result<String, OrchdPersistError> {
    serde_json::to_string(scopes)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize account.scopes: {e}")))
}

fn decode_scopes(s: &str) -> Result<Vec<String>, OrchdPersistError> {
    serde_json::from_str(s)
        .map_err(|e| OrchdPersistError::Io(format!("corrupt account.scopes_json: {e}")))
}

const ACCOUNT_COLUMNS: &str = "id, provider, label, auth_kind, secret_ref, scopes_json, \
     expires_at, refresh_ref, created_at, updated_at";

/// Raw `account` row (text-encoded `auth_kind`, JSON-encoded `scopes`) before decoding into
/// [`AccountRow`] — mirrors `mcp::registry::McpServerRawRow`'s shape.
struct AccountRawRow {
    id: String,
    provider: String,
    label: String,
    auth_kind: String,
    secret_ref: String,
    scopes_json: String,
    expires_at: Option<i64>,
    refresh_ref: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl AccountRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRawRow> {
        Ok(AccountRawRow {
            id: r.get(0)?,
            provider: r.get(1)?,
            label: r.get(2)?,
            auth_kind: r.get(3)?,
            secret_ref: r.get(4)?,
            scopes_json: r.get(5)?,
            expires_at: r.get(6)?,
            refresh_ref: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
        })
    }

    fn into_row(self) -> Result<AccountRow, OrchdPersistError> {
        Ok(AccountRow {
            id: self.id,
            provider: self.provider,
            label: self.label,
            auth_kind: decode_auth_kind(&self.auth_kind)?,
            secret_ref: self.secret_ref,
            scopes: decode_scopes(&self.scopes_json)?,
            expires_at: self.expires_at,
            refresh_ref: self.refresh_ref,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_account(conn: &Connection, id: &str) -> Result<AccountRow, OrchdPersistError> {
    let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM account WHERE id = ?1");
    conn.query_row(&sql, rusqlite::params![id], AccountRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?
        .into_row()
}

/// Deletes the Keychain entry at `r`, treating "already gone" as success (idempotent — a retried
/// `delete_account` after a partial prior failure must not error just because the FIRST attempt
/// already cleaned this particular ref up).
fn delete_secret_ignoring_not_found(r: &bpa_secrets::SecretRef) -> Result<(), OrchdPersistError> {
    match bpa_secrets::delete(r) {
        Ok(()) => Ok(()),
        Err(bpa_secrets::SecretError::NotFound) => Ok(()),
        Err(e) => Err(OrchdPersistError::Io(format!(
            "keychain delete failed: {e}"
        ))),
    }
}

impl Db {
    /// `insert_account` (spec §4 `account` table, task T11). Unlike `add_mcp_server` (which
    /// assigns `id` internally), `new.id` is caller-supplied — see [`NewAccount`]'s doc comment
    /// for why. `created_at`/`updated_at` are still assigned here (`now_ms()`). A `new.id`
    /// collision (astronomically unlikely for a fresh `Uuid::new_v4()`, but a real possible SQLite
    /// outcome) maps to `Conflict` rather than a raw SQL error, mirroring
    /// `map_workspace_conflict`'s precedent.
    pub fn insert_account(&self, new: NewAccount) -> Result<AccountRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let now = now_ms();
        let scopes_json = encode_scopes(&new.scopes)?;
        tx.execute(
            "INSERT INTO account
               (id, provider, label, auth_kind, secret_ref, scopes_json, expires_at, refresh_ref,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            rusqlite::params![
                new.id,
                new.provider,
                new.label,
                encode_auth_kind(&new.auth_kind),
                new.secret_ref,
                scopes_json,
                new.expires_at,
                new.refresh_ref,
                now,
            ],
        )
        .map_err(|e| {
            if is_constraint_violation(&e) {
                OrchdPersistError::Conflict(format!("account {} already exists", new.id))
            } else {
                OrchdPersistError::Sql(e)
            }
        })?;
        let row = load_account(&tx, &new.id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `get_account` (task T11 brief). Unknown `id` ⇒ `NotFound`.
    pub fn get_account(&self, id: &str) -> Result<AccountRow, OrchdPersistError> {
        load_account(self.conn(), id)
    }

    /// `list_accounts` (task T11 brief): every `account` row, creation-ordered (mirrors
    /// `list_mcp_servers`'s ordering convention).
    pub fn list_accounts(&self) -> Result<Vec<AccountRow>, OrchdPersistError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM account ORDER BY created_at, id")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_account(self.conn(), id)).collect()
    }

    /// `delete_account` (task T11 brief: "removes the row AND deletes the Keychain entry").
    /// Keychain cleanup happens BEFORE the SQL delete — fail-closed: a genuine Keychain error
    /// (anything other than "already gone") aborts before the row disappears, so a live secret
    /// never ends up with no DB reference pointing at it. Unknown `id` ⇒ `NotFound`.
    pub fn delete_account(&self, id: &str) -> Result<(), OrchdPersistError> {
        let row = load_account(self.conn(), id)?;
        match row.auth_kind {
            AccountAuthKind::Oauth => {
                delete_secret_ignoring_not_found(&bpa_secrets::account_ref(&row.id, "token"))?;
                if row.refresh_ref.is_some() {
                    delete_secret_ignoring_not_found(&bpa_secrets::account_ref(
                        &row.id, "refresh",
                    ))?;
                }
            }
            AccountAuthKind::Apikey => {
                delete_secret_ignoring_not_found(&bpa_secrets::account_ref(&row.id, "apikey"))?;
            }
        }

        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute("DELETE FROM account WHERE id = ?1", rusqlite::params![id])?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        tx.commit()?;
        Ok(())
    }

    /// `update_account_tokens` (task T11 brief; `ConnectorsState::refresh_oauth_token` calls this
    /// after a successful `exchange_refresh_token`). `refresh_ref` uses the COALESCE idiom
    /// (mirrors `update_mcp_server`): `has_refresh=true` (re)writes the deterministic
    /// `bpa_secrets::account_ref(id, "refresh")` ref string — idempotent, since the Keychain ENTRY
    /// at that ref was already overwritten by the caller before this call runs, whether or not the
    /// IdP actually rotated the refresh token; `has_refresh=false` leaves whatever `refresh_ref`
    /// currently holds untouched (most refresh responses omit `refresh_token` when the IdP doesn't
    /// rotate it — RFC 6749 §6 — so a missing field must never be read as "the refresh token is
    /// gone"). Unknown `id` ⇒ `NotFound`.
    pub fn update_account_tokens(
        &self,
        id: &str,
        expires_at: Option<i64>,
        has_refresh: bool,
    ) -> Result<AccountRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let refresh_ref = has_refresh.then(|| bpa_secrets::account_ref(id, "refresh").account);
        let changed = tx.execute(
            "UPDATE account SET expires_at = ?2, refresh_ref = COALESCE(?3, refresh_ref), \
             updated_at = ?4 WHERE id = ?1",
            rusqlite::params![id, expires_at, refresh_ref, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_account(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;

    #[test]
    fn expires_at_from_saturates_instead_of_overflowing() {
        // A normal ttl adds cleanly.
        assert_eq!(
            expires_at_from(1_000, Duration::from_secs(60)),
            1_000 + 60_000
        );
        // An absurd expires_in (duration_to_ms already clamps to i64::MAX) must NOT wrap `now +
        // i64::MAX` to a negative expiry (permanently-expired) or debug-panic — it saturates (B3).
        let got = expires_at_from(now_ms(), Duration::from_secs(u64::MAX));
        assert_eq!(
            got,
            i64::MAX,
            "a huge ttl saturates to i64::MAX, never negative"
        );
        assert!(got > now_ms(), "a saturated expiry is still in the future");
    }

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn shared_db() -> Arc<TokioMutex<Db>> {
        StdArc::new(TokioMutex::new(new_db()))
    }

    // ---- Keychain skip-guard (mirrors `no_secrets_in_logs_mcp.rs`'s own equivalent — see that
    // file's doc comment: `bpa_secrets`'s precise 4-OSStatus-code probe is `#[cfg(test)]`-private
    // to its own crate, so every OTHER crate's test suite that touches the real Keychain carries
    // this deliberately looser (but equally honest, always-loud) equivalent). ----

    fn keychain_available() -> bool {
        // Hang-proof bounded probe (BL-107): an inline set→get→delete round-trip BLOCKS on a macOS
        // Keychain authorization prompt when this test binary isn't pre-authorized for the login
        // keychain, and a non-interactive shell never answers it — wedging the whole test binary
        // (observed: `connectors::accounts::tests` at 0% CPU for 14+ min). `bpa_secrets::
        // keychain_available` runs the round-trip on a worker thread bounded by a timeout, turning
        // that hang into a loud SKIP instead. The probe itself (service prefix, set→get→delete
        // round-trip, skip-on-unavailable/mismatch) is unchanged — only the hang-proofing moved
        // into the shared helper.
        bpa_secrets::keychain_available(std::time::Duration::from_secs(3))
    }

    #[test]
    fn account_token_debug_redacts_the_bearer() {
        let token = AccountToken {
            bearer: "s3cr3t-bearer-must-not-leak-9f2a".to_string(),
        };
        let rendered = format!("{token:?}");
        assert!(
            !rendered.contains("s3cr3t-bearer-must-not-leak-9f2a"),
            "AccountToken Debug leaked the bearer: {rendered}"
        );
        assert!(
            rendered.contains("REDACTED"),
            "expected redaction marker: {rendered}"
        );
    }

    #[test]
    fn oauth_provider_config_debug_redacts_the_client_secret() {
        let cfg = OAuthProviderConfig {
            client_id: "public-client-id".to_string(),
            client_secret: Some("s3cr3t-client-secret-must-not-leak-c41d".to_string()),
            auth_url: "https://idp.example/authorize".to_string(),
            token_url: "https://idp.example/token".to_string(),
            default_scopes: vec!["read".to_string()],
        };
        let rendered = format!("{cfg:?}");
        assert!(
            !rendered.contains("s3cr3t-client-secret-must-not-leak-c41d"),
            "OAuthProviderConfig Debug leaked the client_secret: {rendered}"
        );
        assert!(
            rendered.contains("REDACTED"),
            "expected redaction marker: {rendered}"
        );
        // Non-secret fields stay visible for debuggability.
        assert!(rendered.contains("public-client-id"));
        assert!(rendered.contains("idp.example/authorize"));
    }

    /// Best-effort teardown so a panicking test never leaves a stray real Keychain entry.
    struct DeleteAccountSecretsOnDrop<'a> {
        account_id: &'a str,
        kinds: &'a [&'a str],
    }
    impl Drop for DeleteAccountSecretsOnDrop<'_> {
        fn drop(&mut self) {
            for kind in self.kinds {
                let _ = bpa_secrets::delete(&bpa_secrets::account_ref(self.account_id, kind));
            }
        }
    }

    // ---- loopback OAuth token-endpoint stub (mirrors `dispatch_integration.rs`'s
    // `spawn_stub_mcp_server` axum/TcpListener wiring — same shape, a different tiny protocol).
    // Branches on the `grant_type` form field so ONE server can answer both the authorization-code
    // exchange AND a later refresh-token exchange with DIFFERENT canned tokens, proving a refresh
    // genuinely round-tripped rather than coincidentally matching the original value. ----

    #[derive(Clone)]
    struct TokenStubResponses {
        initial_access_token: String,
        initial_refresh_token: Option<String>,
        refreshed_access_token: String,
        refreshed_refresh_token: Option<String>,
        expires_in_secs: i64,
    }

    async fn token_stub_handler(
        axum::extract::State(cfg): axum::extract::State<StdArc<TokenStubResponses>>,
        body: axum::body::Bytes,
    ) -> axum::Json<serde_json::Value> {
        let body_str = String::from_utf8_lossy(&body);
        let is_refresh = body_str.contains("grant_type=refresh_token");
        let (access_token, refresh_token) = if is_refresh {
            (
                cfg.refreshed_access_token.clone(),
                cfg.refreshed_refresh_token.clone(),
            )
        } else {
            (
                cfg.initial_access_token.clone(),
                cfg.initial_refresh_token.clone(),
            )
        };
        let mut body = serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": cfg.expires_in_secs,
        });
        if let Some(rt) = refresh_token {
            body["refresh_token"] = serde_json::json!(rt);
        }
        axum::Json(body)
    }

    /// Spawns the token-endpoint stub on an OS-assigned loopback TCP port, returns its
    /// `http://127.0.0.1:<port>/token` URL.
    async fn spawn_token_stub(responses: TokenStubResponses) -> String {
        let state = StdArc::new(responses);
        let router = axum::Router::new()
            .route("/token", axum::routing::post(token_stub_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback token stub");
        let addr = listener.local_addr().expect("stub local_addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}/token")
    }

    fn test_provider_config(token_url: &str) -> OAuthProviderConfig {
        OAuthProviderConfig {
            client_id: "test-client-id".to_string(),
            client_secret: Some("test-client-secret".to_string()),
            auth_url: "https://idp.example.com/authorize".to_string(),
            token_url: token_url.to_string(),
            default_scopes: Vec::new(),
        }
    }

    // ================================================================================
    // ---- `account` CRUD round-trips (no Keychain — pure DB layer) ----
    // ================================================================================

    fn sample_new_account(id: &str) -> NewAccount {
        NewAccount {
            id: id.to_string(),
            provider: "prowl".to_string(),
            label: "My Prowl".to_string(),
            auth_kind: AccountAuthKind::Oauth,
            secret_ref: format!("{id}:token"),
            scopes: vec!["read".to_string(), "write".to_string()],
            expires_at: Some(1_700_000_000_000),
            refresh_ref: Some(format!("{id}:refresh")),
        }
    }

    #[test]
    fn insert_account_round_trips_scopes_and_expires_at() {
        let db = new_db();
        let id = Uuid::new_v4().to_string();
        let row = db.insert_account(sample_new_account(&id)).unwrap();

        assert_eq!(row.id, id);
        assert_eq!(row.provider, "prowl");
        assert_eq!(row.label, "My Prowl");
        assert_eq!(row.auth_kind, AccountAuthKind::Oauth);
        assert_eq!(row.secret_ref, format!("{id}:token"));
        assert_eq!(row.scopes, vec!["read".to_string(), "write".to_string()]);
        assert_eq!(row.expires_at, Some(1_700_000_000_000));
        assert_eq!(row.refresh_ref, Some(format!("{id}:refresh")));
        assert_eq!(row.created_at, row.updated_at);

        assert_eq!(db.get_account(&id).unwrap(), row);
    }

    #[test]
    fn insert_account_apikey_has_no_scopes_expiry_or_refresh() {
        let db = new_db();
        let id = Uuid::new_v4().to_string();
        let new = NewAccount {
            id: id.clone(),
            provider: "generic-rest".to_string(),
            label: "Some API".to_string(),
            auth_kind: AccountAuthKind::Apikey,
            secret_ref: format!("{id}:apikey"),
            scopes: vec![],
            expires_at: None,
            refresh_ref: None,
        };
        let row = db.insert_account(new).unwrap();
        assert_eq!(row.auth_kind, AccountAuthKind::Apikey);
        assert!(row.scopes.is_empty());
        assert_eq!(row.expires_at, None);
        assert_eq!(row.refresh_ref, None);
    }

    #[test]
    fn get_account_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.get_account("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    #[test]
    fn list_accounts_returns_every_row_creation_ordered() {
        let db = new_db();
        let id_a = Uuid::new_v4().to_string();
        let id_b = Uuid::new_v4().to_string();
        db.insert_account(sample_new_account(&id_a)).unwrap();
        db.insert_account(sample_new_account(&id_b)).unwrap();

        let rows = db.list_accounts().unwrap();
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&id_a.as_str()));
        assert!(ids.contains(&id_b.as_str()));
    }

    #[test]
    fn delete_account_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.delete_account("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    #[test]
    fn update_account_tokens_sets_expiry_and_coalesces_refresh_ref() {
        let db = new_db();
        let id = Uuid::new_v4().to_string();
        let mut new = sample_new_account(&id);
        new.refresh_ref = None;
        let row = db.insert_account(new).unwrap();
        assert_eq!(row.refresh_ref, None);

        // has_refresh=false: expires_at updates, refresh_ref stays None (not cleared — there was
        // nothing to clear, and the field must never be misread as "revoked" from a missing one).
        let updated = db.update_account_tokens(&id, Some(42), false).unwrap();
        assert_eq!(updated.expires_at, Some(42));
        assert_eq!(updated.refresh_ref, None);

        // has_refresh=true: refresh_ref becomes the deterministic ref string.
        let updated2 = db.update_account_tokens(&id, Some(99), true).unwrap();
        assert_eq!(updated2.expires_at, Some(99));
        assert_eq!(
            updated2.refresh_ref,
            Some(bpa_secrets::account_ref(&id, "refresh").account)
        );

        // has_refresh=false AGAIN after it was already set: must NOT clear it (COALESCE, not
        // overwrite-with-null).
        let updated3 = db.update_account_tokens(&id, Some(100), false).unwrap();
        assert_eq!(updated3.refresh_ref, updated2.refresh_ref);
    }

    #[test]
    fn update_account_tokens_unknown_id_is_not_found() {
        let db = new_db();
        let err = db
            .update_account_tokens("missing", None, false)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    // ================================================================================
    // ---- begin_oauth (no network, no Keychain) ----
    // ================================================================================

    #[test]
    fn begin_oauth_unknown_provider_is_an_error() {
        let state = ConnectorsState::new();
        let err = state
            .begin_oauth("nope", "label", &[], "http://127.0.0.1:1/callback")
            .unwrap_err();
        assert!(matches!(err, ConnectorError::UnknownProvider(p) if p == "nope"));
    }

    #[test]
    fn begin_oauth_returns_pkce_challenge_url_and_registers_pending() {
        let state = ConnectorsState::new();
        state.register_oauth_provider(
            "test-provider",
            test_provider_config("https://idp.example.com/token"),
        );

        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &["read".to_string(), "write".to_string()],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        assert!(!challenge.state.is_empty());
        let url = oauth2::url::Url::parse(&challenge.authorize_url).unwrap();
        let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();

        assert_eq!(
            pairs.get("client_id").map(String::as_str),
            Some("test-client-id")
        );
        assert_eq!(
            pairs.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:9999/callback")
        );
        assert_eq!(
            pairs.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(
            pairs.get("code_challenge").is_some_and(|c| !c.is_empty()),
            "authorize_url must carry a non-empty code_challenge: {}",
            challenge.authorize_url
        );
        assert_eq!(
            pairs.get("state").map(String::as_str),
            Some(challenge.state.as_str())
        );
        assert!(challenge.authorize_url.contains("code_challenge="));
        assert!(challenge
            .authorize_url
            .contains("code_challenge_method=S256"));
        assert!(challenge
            .authorize_url
            .contains(&format!("state={}", challenge.state)));

        assert!(
            state.has_pending(&challenge.state),
            "begin_oauth must stash a pending entry keyed by the returned state"
        );
    }

    #[test]
    fn provider_names_is_empty_by_default_and_lists_sorted_registered_names() {
        let state = ConnectorsState::new();
        assert!(
            state.provider_names().is_empty(),
            "a fresh registry has no providers"
        );

        // Register out of alphabetical order — provider_names must return them sorted.
        state.register_oauth_provider("prowl", test_provider_config("https://idp.example/token"));
        state.register_oauth_provider("github", test_provider_config("https://idp.example/token"));
        state.register_oauth_provider("azure", test_provider_config("https://idp.example/token"));

        assert_eq!(
            state.provider_names(),
            vec![
                "azure".to_string(),
                "github".to_string(),
                "prowl".to_string()
            ]
        );

        // Re-registering an existing provider overwrites, never duplicates the name.
        state.register_oauth_provider("github", test_provider_config("https://idp.example/token"));
        assert_eq!(state.provider_names().len(), 3);
    }

    #[test]
    fn begin_oauth_falls_back_to_default_scopes_when_caller_passes_none() {
        let state = ConnectorsState::new();
        let mut cfg = test_provider_config("https://idp.example.com/token");
        cfg.default_scopes = vec!["read".to_string(), "write".to_string()];
        state.register_oauth_provider("test-provider", cfg);

        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &[],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        let url = oauth2::url::Url::parse(&challenge.authorize_url).unwrap();
        let scope = url
            .query_pairs()
            .find(|(k, _)| k == "scope")
            .map(|(_, v)| v.into_owned())
            .expect("authorize_url must carry the provider's default scopes");
        assert!(
            scope.contains("read") && scope.contains("write"),
            "got scope={scope}"
        );
    }

    #[test]
    fn begin_oauth_explicit_scopes_win_over_default_scopes() {
        let state = ConnectorsState::new();
        let mut cfg = test_provider_config("https://idp.example.com/token");
        cfg.default_scopes = vec!["read".to_string(), "write".to_string()];
        state.register_oauth_provider("test-provider", cfg);

        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &["admin".to_string()],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        let url = oauth2::url::Url::parse(&challenge.authorize_url).unwrap();
        let scope = url
            .query_pairs()
            .find(|(k, _)| k == "scope")
            .map(|(_, v)| v.into_owned())
            .expect("authorize_url must carry the explicit scope");
        // Explicit "admin" wins; the provider defaults are NOT widened in.
        assert_eq!(
            scope, "admin",
            "explicit caller scopes must not be widened by default_scopes"
        );
    }

    // ================================================================================
    // ---- complete_oauth: unknown state (no network) ----
    // ================================================================================

    #[tokio::test]
    async fn complete_oauth_unknown_state_is_an_error_no_pending() {
        let state = ConnectorsState::new();
        let db = shared_db();
        let err = state
            .complete_oauth(&db, "never-issued-state", "some-code")
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::UnknownState));
    }

    // ================================================================================
    // ---- SSRF guard (D5 locked DoD): the token-exchange HTTP client must NOT follow a
    // redirect from the token endpoint. ----
    // ================================================================================

    /// `/token` replies with a redirect to `/redirected-token`, which WOULD hand back a valid
    /// canned token if the client followed it. `redirect::Policy::none()`
    /// ([`ssrf_guarded_http_client`]) must make the exchange fail instead of silently succeeding
    /// with a token fetched from wherever the redirect pointed — this is exactly the attack shape
    /// the guard exists to close (a malicious/compromised token endpoint redirecting the
    /// authenticated exchange request, including any confidential-client secret, to an
    /// attacker-controlled host). `/redirected-token` answers `any()` (not just `post()`)
    /// DELIBERATELY: an HTTP redirect can cause a client to switch from POST to GET when
    /// following it (status-code dependent — 303 rewrites to GET, 307/308 preserve POST), so
    /// pinning the target to POST-only would let a mutation that DELETES the guard still fail
    /// this test for the wrong reason (405 Method Not Allowed instead of "redirect not followed"),
    /// making the assertion pass vacuously either way. `any()` removes that confound: the ONLY
    /// thing distinguishing "guard present" (exchange fails on the bare redirect response) from
    /// "guard absent" (redirect followed, exchange succeeds with the planted token) is the guard
    /// itself — confirmed by mutation-testing this test against a temporarily-disabled guard.
    async fn spawn_redirecting_token_stub() -> String {
        let router = axum::Router::new()
            .route(
                "/token",
                axum::routing::post(|| async { axum::response::Redirect::to("/redirected-token") }),
            )
            .route(
                "/redirected-token",
                axum::routing::any(|| async {
                    axum::Json(serde_json::json!({
                        "access_token": "must-never-be-reachable-if-guard-works",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback redirecting token stub");
        let addr = listener.local_addr().expect("stub local_addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}/token")
    }

    #[tokio::test]
    async fn complete_oauth_ssrf_guard_does_not_follow_token_endpoint_redirect() {
        let token_url = spawn_redirecting_token_stub().await;

        let state = ConnectorsState::new();
        state.register_oauth_provider("test-provider", test_provider_config(&token_url));
        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &[],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        let db = shared_db();
        let err = state
            .complete_oauth(&db, &challenge.state, "any-code")
            .await
            .unwrap_err();
        assert!(
            matches!(err, ConnectorError::TokenExchange(_)),
            "exchange must fail when the token endpoint tries to redirect (guard must not follow \
             it and must not treat the redirect target's body as the token response), got {err:?}"
        );
    }

    // ================================================================================
    // ---- complete_oauth: real loopback token exchange (Keychain-gated) ----
    // ================================================================================

    #[tokio::test]
    async fn complete_oauth_exchanges_code_stores_tokens_in_keychain_never_in_db() {
        if !keychain_available() {
            return;
        }

        let access_token = "canned-access-token-do-not-leak-7f3a";
        let refresh_token = "canned-refresh-token-do-not-leak-9c21";
        let token_url = spawn_token_stub(TokenStubResponses {
            initial_access_token: access_token.to_string(),
            initial_refresh_token: Some(refresh_token.to_string()),
            refreshed_access_token: "unused-in-this-test".to_string(),
            refreshed_refresh_token: None,
            expires_in_secs: 3600,
        })
        .await;

        let state = ConnectorsState::new();
        state.register_oauth_provider("test-provider", test_provider_config(&token_url));
        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &["read".to_string()],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        let db = shared_db();
        let row = state
            .complete_oauth(&db, &challenge.state, "any-code-the-stub-does-not-validate")
            .await
            .unwrap();

        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &row.id,
            kinds: &["token", "refresh"],
        };

        assert_eq!(row.auth_kind, AccountAuthKind::Oauth);
        assert_eq!(
            row.secret_ref,
            bpa_secrets::account_ref(&row.id, "token").account
        );
        assert_eq!(
            row.refresh_ref,
            Some(bpa_secrets::account_ref(&row.id, "refresh").account)
        );
        assert!(row.expires_at.unwrap() > now_ms());
        assert!(
            !state.has_pending(&challenge.state),
            "pending entry must be consumed"
        );

        // Keychain genuinely holds the tokens.
        let stored_access = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "token")).unwrap();
        assert_eq!(stored_access, access_token.as_bytes());
        let stored_refresh =
            bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "refresh")).unwrap();
        assert_eq!(stored_refresh, refresh_token.as_bytes());

        // The raw token value is NOT in the DB: every TEXT column on the row is either a
        // non-secret field or the deterministic REF string, never the secret bytes themselves.
        for text in [
            row.id.as_str(),
            row.provider.as_str(),
            row.label.as_str(),
            row.secret_ref.as_str(),
            row.refresh_ref.as_deref().unwrap_or(""),
        ] {
            assert!(
                !text.contains(access_token) && !text.contains(refresh_token),
                "account row field leaked a raw token value: {text}"
            );
        }

        // token_for returns the Keychain-backed bearer without a network call (not expired).
        let account_token = state.token_for(&db, &row.id).await.unwrap();
        assert_eq!(account_token.bearer, access_token);
    }

    // ================================================================================
    // ---- add_apikey (Keychain-gated) ----
    // ================================================================================

    #[test]
    fn add_apikey_stores_key_in_keychain_ref_not_value_in_db_list_and_delete_roundtrip() {
        if !keychain_available() {
            return;
        }

        let db = new_db();
        let state = ConnectorsState::new();
        let api_key = "sk-live-do-not-leak-51J3xN9Qz";

        let row = state
            .add_apikey(&db, "generic-rest", "My API", api_key)
            .unwrap();
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &row.id,
            kinds: &["apikey"],
        };

        assert_eq!(row.auth_kind, AccountAuthKind::Apikey);
        assert_eq!(
            row.secret_ref,
            bpa_secrets::account_ref(&row.id, "apikey").account,
            "DB row must store the Keychain REF key, never the api_key value"
        );
        assert_ne!(row.secret_ref, api_key);

        let stored = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "apikey")).unwrap();
        assert_eq!(stored, api_key.as_bytes());

        let listed = db.list_accounts().unwrap();
        assert!(listed.iter().any(|r| r.id == row.id));

        db.delete_account(&row.id).unwrap();
        assert!(db.get_account(&row.id).is_err());
        let after_delete = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "apikey"));
        assert!(
            matches!(after_delete, Err(bpa_secrets::SecretError::NotFound)),
            "delete_account must remove the Keychain entry too: {after_delete:?}"
        );
    }

    #[test]
    fn token_for_apikey_returns_the_stored_key() {
        if !keychain_available() {
            return;
        }
        let db = new_db();
        let state = ConnectorsState::new();
        let row = state
            .add_apikey(&db, "generic-rest", "My API", "the-api-key-value")
            .unwrap();
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &row.id,
            kinds: &["apikey"],
        };

        let db_arc = StdArc::new(TokioMutex::new(db));
        let token = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(state.token_for(&db_arc, &row.id))
            .unwrap();
        assert_eq!(token.bearer, "the-api-key-value");
    }

    // ================================================================================
    // ---- token_for: oauth refresh-on-expiry (Keychain + loopback token stub) ----
    // ================================================================================

    #[tokio::test]
    async fn token_for_refreshes_an_expired_oauth_token_and_persists_the_new_one() {
        if !keychain_available() {
            return;
        }

        let old_access = "old-access-token-8a21";
        let old_refresh = "old-refresh-token-4c19";
        let new_access = "refreshed-access-token-b62d";
        let token_url = spawn_token_stub(TokenStubResponses {
            initial_access_token: old_access.to_string(),
            initial_refresh_token: Some(old_refresh.to_string()),
            refreshed_access_token: new_access.to_string(),
            refreshed_refresh_token: None,
            expires_in_secs: 3600,
        })
        .await;

        let state = ConnectorsState::new();
        state.register_oauth_provider("test-provider", test_provider_config(&token_url));
        let challenge = state
            .begin_oauth(
                "test-provider",
                "My Account",
                &[],
                "http://127.0.0.1:9999/callback",
            )
            .unwrap();

        let db = shared_db();
        let row = state
            .complete_oauth(&db, &challenge.state, "any-code")
            .await
            .unwrap();
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &row.id,
            kinds: &["token", "refresh"],
        };

        // Force the just-inserted row into "already expired" WITHOUT touching Keychain (DB-only
        // mutation) — `has_refresh=true` here just re-asserts the SAME deterministic ref string
        // that's already in Keychain, it does not write new bytes.
        {
            let guard = db.lock().await;
            guard
                .update_account_tokens(&row.id, Some(now_ms() - 1_000), true)
                .unwrap();
        }

        let token = state.token_for(&db, &row.id).await.unwrap();
        assert_eq!(token.bearer, new_access, "must return the REFRESHED bearer");

        let stored_access = bpa_secrets::get(&bpa_secrets::account_ref(&row.id, "token")).unwrap();
        assert_eq!(
            stored_access,
            new_access.as_bytes(),
            "Keychain must hold the new token"
        );

        let refreshed_row = { db.lock().await.get_account(&row.id).unwrap() };
        assert!(
            refreshed_row.expires_at.unwrap() > now_ms(),
            "expires_at must be pushed back into the future"
        );
        // The stub's refresh response omitted `refresh_token` — the OLD refresh_ref must survive
        // (COALESCE idiom: a missing field never reads as "revoked").
        assert_eq!(refreshed_row.refresh_ref, row.refresh_ref);
    }

    #[tokio::test]
    async fn token_for_oauth_not_expired_returns_cached_token_without_network() {
        if !keychain_available() {
            return;
        }

        let access_token = "not-expired-access-token-1a2b";
        let db = new_db();
        let id = Uuid::new_v4().to_string();
        bpa_secrets::set(
            &bpa_secrets::account_ref(&id, "token"),
            access_token.as_bytes(),
        )
        .unwrap();
        let _cleanup = DeleteAccountSecretsOnDrop {
            account_id: &id,
            kinds: &["token"],
        };
        db.insert_account(NewAccount {
            id: id.clone(),
            provider: "test-provider".to_string(),
            label: "My Account".to_string(),
            auth_kind: AccountAuthKind::Oauth,
            secret_ref: bpa_secrets::account_ref(&id, "token").account,
            scopes: vec![],
            expires_at: Some(now_ms() + 3_600_000),
            refresh_ref: None,
        })
        .unwrap();

        // No provider registered at all — if this path tried to refresh (a bug), `provider_config`
        // would fail with `UnknownProvider`, so a *successful* `token_for` here proves the
        // no-network cached-token path was actually taken, not just "didn't crash".
        let state = ConnectorsState::new();
        let db_arc = StdArc::new(TokioMutex::new(db));
        let token = state.token_for(&db_arc, &id).await.unwrap();
        assert_eq!(token.bearer, access_token);
    }

    /// BL-91 (spec D5): the token-exchange/refresh HTTP client must NOT hang on a peer that
    /// accepts the TCP connection but never sends an HTTP response. A bound-but-unaccepting
    /// listener completes the TCP handshake (OS backlog) yet returns no bytes, so only the request
    /// timeout can end the call.
    #[tokio::test]
    async fn oauth_http_client_times_out_on_a_silent_peer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // The listener stays bound (so connect succeeds) but is never `accept()`ed / answered.
        let client =
            ssrf_guarded_http_client_with_timeout(std::time::Duration::from_millis(200)).unwrap();
        let url = format!("http://{addr}/token");

        // Outer guard: a regression that drops the timeout would hang HERE, not the whole suite.
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(5), client.get(&url).send())
                .await
                .expect("the request must return within the client timeout, not hang");

        assert!(
            result.is_err(),
            "a silent peer must produce a client error (timeout), got {result:?}"
        );
    }
}
