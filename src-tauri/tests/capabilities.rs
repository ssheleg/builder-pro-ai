//! Validates `capabilities/default.json` (spec §6/§16) and `entitlements.plist` (spec §14.3)
//! without needing a Tauri runtime: both are plain data files, `include_str!`-ed at compile time
//! so a missing/malformed file fails the build itself, not just a test assertion.

use serde_json::Value;

fn load_caps() -> Value {
    let raw = include_str!("../capabilities/default.json");
    serde_json::from_str(raw).expect("capabilities/default.json must be valid JSON")
}

fn perm_ids(caps: &Value) -> Vec<String> {
    caps["permissions"]
        .as_array()
        .expect("permissions is an array")
        .iter()
        .map(|p| match p {
            Value::String(s) => s.clone(),
            Value::Object(o) => o["identifier"].as_str().unwrap_or_default().to_string(),
            _ => panic!("permission entry must be a string or an object with identifier"),
        })
        .collect()
}

#[test]
fn capabilities_parse_and_target_main_window() {
    let caps = load_caps();
    assert_eq!(caps["identifier"], "default");
    let windows = caps["windows"].as_array().expect("windows array");
    assert!(
        windows.iter().any(|w| w == "main"),
        "capability must apply to the main window"
    );
}

#[test]
fn capabilities_grant_minimal_required_permissions() {
    let caps = load_caps();
    let ids = perm_ids(&caps);
    for required in [
        "core:default",
        "dialog:default",
        "dialog:allow-open",
        "shell:default",
    ] {
        assert!(
            ids.iter().any(|i| i == required),
            "capabilities must grant {required}; got {ids:?}"
        );
    }
}

#[test]
fn capabilities_do_not_grant_removed_store_or_fs_plugins() {
    // 2026-07-24 audit (FE-9/FE-5): the store/fs plugins were dead code — nothing in the
    // webview called them — and their ACL surface (store:default, fs:default + a broad
    // $APPDATA/** scope) was pure risk. The plugins were dropped from Cargo.toml/lib.rs, so
    // their permissions must NOT come back here either.
    let caps = load_caps();
    let ids = perm_ids(&caps);
    for forbidden in ["store:default", "fs:default", "fs:scope"] {
        assert!(
            !ids.iter().any(|i| i == forbidden),
            "capabilities must NOT grant {forbidden} (FE-9/FE-5: store/fs plugins removed)"
        );
    }
}

#[test]
fn capabilities_do_not_grant_dangerous_shell_execute_scopes() {
    // shell:default is retained only for tauri-plugin-shell's runtime init (spec §6); the daemon
    // is launchd-managed (spec §8.3), never shell-spawned by the webview. We must NOT hand the
    // webview arbitrary shell exec / spawn.
    let caps = load_caps();
    let ids = perm_ids(&caps);
    for forbidden in ["shell:allow-execute", "shell:allow-spawn"] {
        assert!(
            !ids.iter().any(|i| i == forbidden),
            "capabilities must NOT grant {forbidden}"
        );
    }
}

#[test]
fn entitlements_plist_has_hardened_runtime_keys() {
    let raw = include_str!("../entitlements.plist");
    assert!(raw.contains("<!DOCTYPE plist"), "must be a plist doctype");
    assert!(raw.contains("<plist"), "must have a <plist> root");
    // Hardened runtime for a WKWebView app that also embeds signed sidecars (spec §14.3): the app
    // uses JS, so it needs the JIT / unsigned-executable-memory / library-validation exceptions
    // WKWebView + the deep-signed daemon sidecars require, plus allow-dyld-environment-variables
    // for the sidecar launch. (BL-174 tried to drop the latter two but the narrowed set broke
    // deep-sign/notarization; all four restored.)
    for key in [
        "com.apple.security.cs.allow-jit",
        "com.apple.security.cs.allow-unsigned-executable-memory",
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.cs.allow-dyld-environment-variables",
    ] {
        assert!(raw.contains(key), "entitlements must declare {key}");
    }
    // The app is deliberately NOT sandboxed — it orchestrates real terminals and spawns the two
    // launchd daemons — so the sandbox keys are intentionally ABSENT. `com.apple.security.inherit`
    // is a sandbox-container inheritance key (meaningless without `app-sandbox`) and, under the
    // hardened runtime, its presence broke AMFI codesigning (removed in the release fix, 63c0397).
    for absent in [
        "com.apple.security.app-sandbox",
        "com.apple.security.inherit",
    ] {
        assert!(
            !raw.contains(absent),
            "entitlements must NOT declare the sandbox key {absent} (the app is not sandboxed)"
        );
    }
}
