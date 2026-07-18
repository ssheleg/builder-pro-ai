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
        "store:default",
        "dialog:default",
        "dialog:allow-open",
        "fs:default",
        "shell:default",
    ] {
        assert!(
            ids.iter().any(|i| i == required),
            "capabilities must grant {required}; got {ids:?}"
        );
    }
}

#[test]
fn capabilities_grant_scoped_fs_access_under_appdata() {
    let caps = load_caps();
    let perms = caps["permissions"].as_array().expect("permissions array");
    let scope = perms
        .iter()
        .find(|p| matches!(p, Value::Object(o) if o.get("identifier").and_then(Value::as_str) == Some("fs:scope")))
        .expect("must grant a scoped fs:scope permission (not unbounded fs access)");
    let allow = scope["allow"].as_array().expect("fs:scope.allow array");
    assert!(
        allow.iter().any(|a| a["path"] == "$APPDATA/**"),
        "fs:scope must allow $APPDATA/**; got {allow:?}"
    );
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
    // WKWebView requires, plus allow-dyld-environment-variables for the sidecar launch.
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
