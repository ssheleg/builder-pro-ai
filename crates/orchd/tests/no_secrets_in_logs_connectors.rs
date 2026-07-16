//! No-secrets-in-logs test for the config-backed OAuth provider registry loader (spec D7, O-5;
//! spec §9's "no-secrets-in-logs for MCP/connector surface"). The `oauth_providers.json` file MAY
//! carry a confidential-client `client_secret`; `registry_config::load_oauth_providers` runs at
//! boot and MUST never let that value into the structured log — it logs only the provider COUNT and
//! NAMES (public OAuth-registration values), with `OAuthProviderConfig`'s redacting `Debug` as the
//! type-level backstop.
//!
//! Mirrors `no_secrets_in_logs.rs`'s shape (plant a secret, drive real code against a real tracing
//! sink, assert absence) and is Keychain-free — the loader touches no Keychain, so this needs no
//! probe/skip. Non-vacuous by construction: the SAME assertion block that checks the secret is
//! ABSENT also checks the provider NAME is PRESENT, proving the loader actually ran and logged
//! (rather than passing because nothing was emitted at all).

use std::fs;
use std::io::Read;

use bpa_orchd::connectors::accounts::ConnectorsState;
use bpa_orchd::connectors::registry_config::load_oauth_providers;

#[test]
fn planted_client_secret_never_appears_in_logs_but_provider_name_does() {
    let secret = "s3cr3t-CLIENT-SECRET-must-not-leak-in-registry-load-91af";
    let provider_name = "prowl";

    // Single test in this file/binary — `cargo test` runs each integration-test FILE as its own
    // process, so this is the only `init_to_file` call in this process (its one-per-process rule
    // holds). Mirrors `no_secrets_in_logs.rs`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("orchd.test.log");
    let app_support = tmp.path().join("app-support");
    fs::create_dir_all(&app_support).expect("app-support dir");

    bpa_daemon_core::logging::init_to_file(&log_path).expect("init logging");

    // Plant a config carrying a live client_secret, then drive the REAL loader (the exact function
    // `boot::run` calls) against it.
    fs::write(
        app_support.join("oauth_providers.json"),
        format!(
            r#"{{
              "{provider_name}": {{
                "client_id": "prowl-client-id",
                "auth_url": "https://prowl.chat/oauth/authorize",
                "token_url": "https://prowl.chat/oauth/token",
                "default_scopes": ["read"],
                "client_secret": "{secret}"
              }}
            }}"#
        ),
    )
    .expect("write config");

    let state = ConnectorsState::new();
    load_oauth_providers(&state, &app_support);

    // The provider genuinely registered (the load was real, not a no-op).
    assert_eq!(state.provider_names(), vec![provider_name.to_string()]);

    bpa_daemon_core::logging::flush();
    let mut log = String::new();
    fs::File::open(&log_path)
        .expect("open log")
        .read_to_string(&mut log)
        .expect("read log");

    // Non-vacuous: the loader DID log (the provider name / count line is present) ...
    assert!(
        log.contains(provider_name),
        "expected the loader to log the provider name (proves it ran); log:\n{log}"
    );
    // ... AND the planted client_secret is nowhere in it.
    assert!(
        !log.contains(secret),
        "client_secret leaked into the log during registry load:\n{log}"
    );
}
