//! Shared `DYLD_*`/`LD_*` dynamic-linker-injection env denylist (S-EXT spec §6, D10, task T16;
//! closes the long-open BL-1 in `bpa-sessiond`). A child process that inherits `DYLD_INSERT_LIBRARIES`
//! / `DYLD_LIBRARY_PATH` (macOS `dyld`) or `LD_PRELOAD` / `LD_LIBRARY_PATH` (Linux/glibc `ld.so`)
//! can have arbitrary code injected into it at load time — these are dynamic-linker injection
//! vectors, not ordinary configuration. ANY daemon that spawns a child process with a
//! caller-influenced env (sessiond's terminal sessions via `env_overrides`; orchd's stdio MCP
//! servers, spec D6) must strip them before the child ever sees that env. One shared helper here,
//! used by BOTH spawn paths, so "strip the linker-injection vars" is implemented exactly once
//! rather than risking a second, silently-unfiltered copy (spec §6: "a second unfiltered spawn
//! path must not exist").
//!
//! Case-sensitive prefix match: the real linker variables are always upper-case
//! (`DYLD_INSERT_LIBRARIES`, `LD_PRELOAD`, ...) — dyld/ld.so only ever recognize the upper-case
//! spelling, so a lower-case look-alike (`dyld_foo`, `ld_foo`) is not a linker variable at all and
//! is deliberately left alone; stripping it would be over-broad and could clobber an unrelated
//! caller-defined variable that merely happens to share a casually-lower-cased prefix.

use std::collections::BTreeMap;

/// Remove every `(key, value)` pair from `pairs` whose key starts with `DYLD_` or `LD_`
/// (case-sensitive — see module doc). Mutates in place; order of the surviving pairs is
/// preserved.
pub fn strip_dangerous_env(pairs: &mut Vec<(String, String)>) {
    pairs.retain(|(k, _)| !is_dangerous_key(k));
}

/// [`strip_dangerous_env`]'s `BTreeMap` convenience for callers whose env is already keyed
/// (e.g. orchd's `mcp_server.env` column, spec §4) — avoids every such caller hand-rolling its
/// own `Vec<->BTreeMap` round-trip just to reuse the `Vec` primitive above.
pub fn strip_dangerous_env_map(env: &mut BTreeMap<String, String>) {
    env.retain(|k, _| !is_dangerous_key(k));
}

/// The actual denylist predicate: a `DYLD_*` (macOS dyld) or `LD_*` (Linux/glibc ld.so) prefix,
/// case-sensitive (module doc: real linker vars are always upper-case).
fn is_dangerous_key(key: &str) -> bool {
    key.starts_with("DYLD_") || key.starts_with("LD_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn strips_known_dyld_and_ld_injection_vars() {
        let mut env = pairs(&[
            ("DYLD_INSERT_LIBRARIES", "/evil.dylib"),
            ("DYLD_LIBRARY_PATH", "/evil"),
            ("LD_PRELOAD", "/evil.so"),
            ("LD_LIBRARY_PATH", "/evil"),
            ("PATH", "/usr/bin:/bin"),
        ]);
        strip_dangerous_env(&mut env);

        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec!["PATH"],
            "only the benign PATH entry must survive"
        );
    }

    #[test]
    fn keeps_benign_vars_untouched() {
        let mut env = pairs(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/x"),
            ("FOO", "bar"),
        ]);
        let before = env.clone();
        strip_dangerous_env(&mut env);
        assert_eq!(env, before, "no benign var should ever be removed");
    }

    #[test]
    fn lowercase_lookalikes_are_not_stripped_case_sensitive() {
        // Real dyld/ld.so only ever recognize the upper-case spelling — a lower-case key is not
        // a linker variable and must be left alone (module doc: over-broad stripping would
        // clobber an unrelated caller-defined variable).
        let mut env = pairs(&[("dyld_foo", "keepme"), ("ld_bar", "keepme")]);
        let before = env.clone();
        strip_dangerous_env(&mut env);
        assert_eq!(
            env, before,
            "lower-case look-alikes are not linker vars, must be kept"
        );
    }

    #[test]
    fn map_variant_strips_the_same_keys() {
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        env.insert(
            "DYLD_INSERT_LIBRARIES".to_string(),
            "/evil.dylib".to_string(),
        );
        env.insert("LD_PRELOAD".to_string(), "/evil.so".to_string());
        env.insert("FOO".to_string(), "bar".to_string());

        strip_dangerous_env_map(&mut env);

        assert_eq!(env.len(), 1);
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
    }
}
