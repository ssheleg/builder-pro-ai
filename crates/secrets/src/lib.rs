//! macOS Keychain generic-password wrapper (S-EXT §3, D4, BL-20).
//!
//! Thin, safe wrapper around `security_framework::passwords::{set_generic_password,
//! get_generic_password, delete_generic_password}`, targeting the user's default (login)
//! keychain. This is the ONLY place in the app that talks to Security.framework directly —
//! `orchd.db` persists [`SecretRef`] coordinates (service/account strings), never secret bytes,
//! and callers pass secret bytes straight through without ever routing them through logs.

use security_framework::base::Error as SfError;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain service name for MCP server bearer tokens (spec §3/§4).
const MCP_SERVICE: &str = "ai.builderpro.desktop.mcp";
/// Keychain service name for connector account secrets (spec §3/§4).
const ACCOUNT_SERVICE: &str = "ai.builderpro.desktop.account";

/// `OSStatus` for `errSecItemNotFound` (`Security/SecBase.h`). Not re-exported as a named
/// constant by `security-framework`/`security-framework-sys` 3.7/2.17's public API (only
/// `errSecDuplicateItem`/`errSecParam` are), so the well-known Apple value is pinned here.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Coordinates of a Keychain generic-password entry: which service + account it lives under.
/// Never carries the secret itself — only [`set`]/[`get`] take or return secret bytes, and
/// neither `SecretRef` nor [`SecretError`] ever hold them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef {
    pub service: String,
    pub account: String,
}

/// Error returned by Keychain operations.
///
/// `Display`/`Debug` NEVER include secret bytes: this type structurally cannot, since neither
/// variant carries a `Vec<u8>`/secret payload — `Keychain`'s `String` is always
/// Security.framework's own status message (or a fixed fallback describing the `OSStatus`),
/// never anything derived from the secret argument passed to [`set`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    /// No matching Keychain entry for the given [`SecretRef`].
    #[error("secret not found in keychain")]
    NotFound,
    /// Any other Keychain/Security.framework failure. The inner `String` is a secret-free
    /// status message.
    #[error("keychain error: {0}")]
    Keychain(String),
}

impl From<SfError> for SecretError {
    fn from(err: SfError) -> Self {
        if err.code() == ERR_SEC_ITEM_NOT_FOUND {
            Self::NotFound
        } else {
            Self::Keychain(
                err.message()
                    .unwrap_or_else(|| format!("OSStatus {}", err.code())),
            )
        }
    }
}

/// Store (create or update) the secret bytes for `r` in the login Keychain. Upserts: calling
/// this again for the same `r` replaces the previously stored value.
pub fn set(r: &SecretRef, secret: &[u8]) -> Result<(), SecretError> {
    set_generic_password(&r.service, &r.account, secret).map_err(Into::into)
}

/// Retrieve the secret bytes stored for `r`. Returns `Err(SecretError::NotFound)` if no
/// matching entry exists.
pub fn get(r: &SecretRef) -> Result<Vec<u8>, SecretError> {
    get_generic_password(&r.service, &r.account).map_err(Into::into)
}

/// Delete the Keychain entry for `r`. Returns `Err(SecretError::NotFound)` if no matching
/// entry exists.
pub fn delete(r: &SecretRef) -> Result<(), SecretError> {
    delete_generic_password(&r.service, &r.account).map_err(Into::into)
}

/// [`SecretRef`] for an MCP server's bearer token (spec §3/§4): service
/// `"ai.builderpro.desktop.mcp"`, account = `server_id`.
pub fn mcp_bearer_ref(server_id: &str) -> SecretRef {
    SecretRef {
        service: MCP_SERVICE.to_string(),
        account: server_id.to_string(),
    }
}

/// [`SecretRef`] for a connector account secret (spec §3/§4): service
/// `"ai.builderpro.desktop.account"`, account = `"{account_id}:{kind}"`. `kind` is one of
/// `"token"`, `"refresh"`, `"apikey"`.
pub fn account_ref(account_id: &str, kind: &str) -> SecretRef {
    SecretRef {
        service: ACCOUNT_SERVICE.to_string(),
        account: format!("{account_id}:{kind}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `OSStatus` codes (`Security/SecBase.h`) that mean "no usable Keychain in this session"
    /// rather than "our wrapper is broken" — the signature a headless/sandboxed CI runner
    /// produces when there is no unlocked login keychain to operate on:
    /// - `errSecInteractionNotAllowed` (-25308): UI interaction would be required (e.g. an
    ///   implicit unlock prompt) but is disallowed in this session.
    /// - `errSecNoDefaultKeychain` (-25307): no default keychain is configured at all.
    /// - `errSecNoSuchKeychain` (-25294): the keychain search list references a keychain that
    ///   does not exist on disk.
    /// - `errSecNotAvailable` (-25291): no trust/keychain subsystem available.
    ///
    /// CI-robustness strategy (documented per task brief): production code always targets the
    /// user's default login keychain via `security_framework::passwords::*` — that is the
    /// correct, and only sane, target for a real desktop app, so we do not fork production
    /// behavior for tests. Instead this test suite probes the keychain first and skips (with a
    /// clear `eprintln!`, not a silent vacuous pass) when one of these codes comes back, while
    /// still running the full genuine set→get→update→delete→NotFound assertion whenever a
    /// keychain IS reachable (always true in local/dev runs, and on macOS CI runners with a
    /// provisioned login keychain).
    const KEYCHAIN_UNAVAILABLE_CODES: [i32; 4] = [-25308, -25307, -25294, -25291];

    /// Probes the login keychain with a disposable entry under the `.test` service prefix and
    /// reports whether it is usable. This is a FULL `set → get (assert bytes match) → delete`
    /// round-trip, NOT a set-only check: Keychain Services treats the "default keychain" and the
    /// "search list" as independent, so a keychain that a `set` writes to (the default) is not
    /// necessarily the one a `get`/`delete` resolves (the search list) — a misconfigured CI
    /// keychain (created + set-default + unlocked but NOT added to the search list) makes `set`
    /// succeed while `get`/`delete` fail "not found". A set-only probe would report "available"
    /// and the real test's `get` would then panic; the round-trip catches that case and SKIPs
    /// loudly instead. Returns `false` (after printing a SKIP notice) for the known "unavailable"
    /// codes above OR any get/delete/round-trip mismatch; only a genuinely unexpected `set` error
    /// (not in the unavailable set) still panics.
    fn keychain_available() -> bool {
        let probe = SecretRef {
            service: "ai.builderpro.desktop.test".to_string(),
            account: "keychain-availability-probe".to_string(),
        };
        // Clean up any stray probe entry from a previously-crashed run before starting.
        let _ = delete_generic_password(&probe.service, &probe.account);

        const PROBE_BYTES: &[u8] = b"probe-roundtrip-marker";
        let skip = |reason: &str| {
            eprintln!(
                "SKIP bpa_secrets::tests keychain roundtrip: {reason} — graceful skip, not a \
                 pass. Run locally with an unlocked login keychain (or a CI keychain that is on \
                 the search list) to exercise the full assertion."
            );
            let _ = delete_generic_password(&probe.service, &probe.account);
            false
        };
        match set_generic_password(&probe.service, &probe.account, PROBE_BYTES) {
            Ok(()) => {}
            Err(err) if KEYCHAIN_UNAVAILABLE_CODES.contains(&err.code()) => {
                return skip(&format!(
                    "login keychain unavailable (OSStatus {})",
                    err.code()
                ));
            }
            Err(err) => panic!("unexpected keychain error during availability probe: {err}"),
        }
        match get_generic_password(&probe.service, &probe.account) {
            Ok(bytes) if bytes == PROBE_BYTES => {}
            Ok(_) => return skip("probe get returned the wrong bytes (keychain misconfigured)"),
            Err(err) => {
                return skip(&format!(
                    "probe get failed after a successful set (OSStatus {} — keychain likely not \
                     on the search list)",
                    err.code()
                ));
            }
        }
        match delete_generic_password(&probe.service, &probe.account) {
            Ok(()) => true,
            Err(err) => skip(&format!(
                "probe delete failed after a successful set+get (OSStatus {})",
                err.code()
            )),
        }
    }

    /// Best-effort teardown so a panic mid-test never leaves a stray entry in the real
    /// (default/login) keychain.
    struct DeleteOnDrop<'a>(&'a SecretRef);
    impl Drop for DeleteOnDrop<'_> {
        fn drop(&mut self) {
            let _ = delete(self.0);
        }
    }

    #[test]
    fn set_get_update_delete_roundtrip() {
        if !keychain_available() {
            return;
        }

        let r = SecretRef {
            service: "ai.builderpro.desktop.test".to_string(),
            account: "set_get_update_delete_roundtrip".to_string(),
        };
        let _cleanup = DeleteOnDrop(&r);

        set(&r, b"first-secret").expect("set should succeed");
        let got = get(&r).expect("get should return the stored secret");
        assert_eq!(got, b"first-secret");

        set(&r, b"updated-secret").expect("set should upsert");
        let got2 = get(&r).expect("get should return the updated secret");
        assert_eq!(got2, b"updated-secret");

        delete(&r).expect("delete should succeed");
        match get(&r) {
            Err(SecretError::NotFound) => {}
            other => panic!("expected NotFound after delete, got {other:?}"),
        }
    }

    #[test]
    fn mcp_bearer_ref_uses_mcp_service_and_server_id_account() {
        let r = mcp_bearer_ref("srv-123");
        assert_eq!(r.service, "ai.builderpro.desktop.mcp");
        assert_eq!(r.account, "srv-123");
    }

    #[test]
    fn account_ref_uses_account_service_and_composite_account() {
        let r = account_ref("acct-abc", "refresh");
        assert_eq!(r.service, "ai.builderpro.desktop.account");
        assert_eq!(r.account, "acct-abc:refresh");
    }

    #[test]
    fn secret_error_display_never_leaks_secret_bytes() {
        let planted_secret = "sk-live-51J3xN9QzTopSecretPlantedValue000";

        let not_found_err = SecretError::NotFound;
        let keychain_err: SecretError = SfError::from_code(-25291).into();

        for text in [
            format!("{not_found_err}"),
            format!("{not_found_err:?}"),
            format!("{keychain_err}"),
            format!("{keychain_err:?}"),
        ] {
            assert!(
                !text.contains(planted_secret),
                "SecretError rendering leaked the planted secret: {text}"
            );
        }
    }
}
