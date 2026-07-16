//! Config-backed OAuth provider registry loader (spec D7, objective O-5, S-POLISH P4).
//!
//! At boot ([`crate::boot::run`]) the daemon reads `<app-support>/oauth_providers.json` and
//! registers every provider it declares into [`ConnectorsState`]'s in-memory OAuth provider
//! registry (via `register_oauth_provider`). This is the "real IdP config" seam the
//! `connectors::accounts` module doc comment reserved for a later task — before it, v1 booted with
//! an empty registry and every `ConnectorBeginOAuth` failed with `UnknownProvider`; with it, an
//! owner drops a JSON file next to the daemon's other durable state (spec §5: `{app-support}/`) and
//! their providers become reachable, activating the D5 OAuth-exchange timeout on a now-reachable
//! path.
//!
//! **Honest degradation (spec D7, matching `boot::open_db_degrading`/`ensure_global_ruleset`'s
//! stance):** this is supplementary boot state, never a boot-blocking dependency. A MISSING file is
//! the normal v1 default — info-logged, empty registry, `run` proceeds. A MALFORMED file (unreadable
//! bytes, or JSON that doesn't match the schema) is error-logged and leaves the registry empty; the
//! daemon still boots fully. Neither case returns an error or panics.
//!
//! **No-secrets discipline (spec D7, D4):** the file MAY carry a confidential-client `client_secret`.
//! This loader NEVER logs the parsed config — only the provider COUNT and the provider NAMES (public
//! OAuth-registration values, the same names that already cross the wire via `ConnectorListProviders`).
//! [`OAuthProviderConfig`]'s own redacting `Debug` is the backstop if a config value is ever
//! `{:?}`-formatted downstream.

use std::collections::HashMap;
use std::path::Path;

use super::accounts::{ConnectorsState, OAuthProviderConfig};

/// File name of the OAuth provider registry config, under `{app-support}/` (spec D7).
pub const OAUTH_PROVIDERS_FILE: &str = "oauth_providers.json";

/// One provider entry as it appears in `oauth_providers.json` (spec D7 shape:
/// `{ "<provider>": { "client_id", "auth_url", "token_url", "default_scopes"?: [..],
/// "client_secret"? } }`). Deliberately NOT `Debug`/`Serialize` — a `client_secret`-bearing value
/// must have no easy path into a log line; the loader logs only names, never this struct.
/// `deny_unknown_fields` makes a typo'd key (e.g. `clientId` instead of `client_id`) a loud
/// malformed-file error rather than a silently-ignored field that yields a subtly-wrong provider.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderConfig {
    client_id: String,
    auth_url: String,
    token_url: String,
    #[serde(default)]
    default_scopes: Option<Vec<String>>,
    #[serde(default)]
    client_secret: Option<String>,
}

/// Load `<app_support>/oauth_providers.json` into `state`'s OAuth provider registry (spec D7).
/// Idempotent w.r.t. `register_oauth_provider` (last write per provider name wins). Missing file ⇒
/// empty registry + info log; malformed file ⇒ empty registry + error log; both leave the daemon
/// bootable (never returns/panics). See the module doc comment for the full degradation contract.
pub fn load_oauth_providers(state: &ConnectorsState, app_support: &Path) {
    let path = app_support.join(OAUTH_PROVIDERS_FILE);

    let raw = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                path = %path.display(),
                "no oauth_providers.json; OAuth provider registry is empty (no providers configured)"
            );
            return;
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                path = %path.display(),
                "failed to read oauth_providers.json; OAuth provider registry left empty (daemon still boots)"
            );
            return;
        }
    };

    let parsed: HashMap<String, RawProviderConfig> = match serde_json::from_str(&raw) {
        Ok(map) => map,
        Err(e) => {
            tracing::error!(
                error = %e,
                path = %path.display(),
                "malformed oauth_providers.json; OAuth provider registry left empty (daemon still boots)"
            );
            return;
        }
    };

    let mut names: Vec<String> = Vec::with_capacity(parsed.len());
    for (provider, cfg) in parsed {
        state.register_oauth_provider(
            &provider,
            OAuthProviderConfig {
                client_id: cfg.client_id,
                client_secret: cfg.client_secret,
                auth_url: cfg.auth_url,
                token_url: cfg.token_url,
                default_scopes: cfg.default_scopes.unwrap_or_default(),
            },
        );
        names.push(provider);
    }
    names.sort();

    tracing::info!(
        count = names.len(),
        providers = ?names,
        path = %path.display(),
        "loaded OAuth provider registry from oauth_providers.json"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal, hermetic app-support dir (a fresh tempdir) — the loader takes the path directly,
    /// so no `$HOME` redirection is needed here (unlike the dispatch integration tests).
    fn write_providers_file(dir: &Path, contents: &str) {
        std::fs::write(dir.join(OAUTH_PROVIDERS_FILE), contents).unwrap();
    }

    #[test]
    fn missing_file_leaves_registry_empty_and_does_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        // No oauth_providers.json written at all.
        load_oauth_providers(&state, dir.path());
        assert!(
            state.provider_names().is_empty(),
            "a missing config file must leave the registry empty"
        );
    }

    #[test]
    fn valid_file_registers_every_provider_and_they_are_retrievable() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        write_providers_file(
            dir.path(),
            r#"{
              "prowl": {
                "client_id": "prowl-client",
                "auth_url": "https://prowl.chat/oauth/authorize",
                "token_url": "https://prowl.chat/oauth/token",
                "default_scopes": ["read", "write"],
                "client_secret": "prowl-secret"
              },
              "github": {
                "client_id": "gh-client",
                "auth_url": "https://github.com/login/oauth/authorize",
                "token_url": "https://github.com/login/oauth/access_token"
              }
            }"#,
        );

        load_oauth_providers(&state, dir.path());

        // Names are registered (sorted).
        assert_eq!(
            state.provider_names(),
            vec!["github".to_string(), "prowl".to_string()]
        );

        // The full config genuinely round-tripped: a `begin_oauth` against a registered provider
        // succeeds (proving auth_url/client_id were stored), and picks up the provider's
        // default_scopes when the caller passes none.
        let challenge = state
            .begin_oauth("prowl", "My Prowl", &[], "http://127.0.0.1:9999/callback")
            .unwrap();
        let url = oauth2::url::Url::parse(&challenge.authorize_url).unwrap();
        let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.get("client_id").map(String::as_str),
            Some("prowl-client")
        );
        let scope = pairs.get("scope").cloned().unwrap_or_default();
        assert!(
            scope.contains("read") && scope.contains("write"),
            "got scope={scope}"
        );
    }

    #[test]
    fn malformed_json_leaves_registry_empty_and_daemon_boots() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        write_providers_file(dir.path(), "{ this is not valid json ]");

        // Must not panic; must leave the registry empty.
        load_oauth_providers(&state, dir.path());
        assert!(
            state.provider_names().is_empty(),
            "a malformed config file must leave the registry empty, never crash the boot"
        );
    }

    #[test]
    fn schema_mismatch_missing_required_field_is_treated_as_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        // Valid JSON, but the provider entry omits the required `token_url`.
        write_providers_file(
            dir.path(),
            r#"{ "prowl": { "client_id": "x", "auth_url": "https://idp/authorize" } }"#,
        );

        load_oauth_providers(&state, dir.path());
        assert!(
            state.provider_names().is_empty(),
            "a schema-mismatched (missing required field) config must leave the registry empty"
        );
    }

    #[test]
    fn unknown_field_typo_is_rejected_as_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        // `clientId` (camelCase typo) instead of `client_id` — deny_unknown_fields catches it.
        write_providers_file(
            dir.path(),
            r#"{ "prowl": { "clientId": "x", "auth_url": "https://idp/a", "token_url": "https://idp/t" } }"#,
        );

        load_oauth_providers(&state, dir.path());
        assert!(
            state.provider_names().is_empty(),
            "an unknown/typo'd field must be a loud malformed-file error, not a silently-dropped field"
        );
    }

    #[test]
    fn empty_json_object_is_a_valid_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let state = ConnectorsState::new();
        write_providers_file(dir.path(), "{}");

        load_oauth_providers(&state, dir.path());
        assert!(state.provider_names().is_empty());
    }
}
