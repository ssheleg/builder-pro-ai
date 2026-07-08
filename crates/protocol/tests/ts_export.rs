use std::fs;
use std::path::PathBuf;

use bpa_protocol::*;
use ts_rs::TS;

/// Absolute path to the generated shared TS file (repo root `src/ipc/types.ts`).
fn types_ts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/ipc")
}

fn types_ts_path() -> PathBuf {
    types_ts_dir().join("types.ts")
}

/// Force every exported type to (re)write its TS binding, then read the file back.
/// `export_all_to` disregards `TS_RS_EXPORT_DIR`, so the output directory is
/// deterministic regardless of environment or working directory. Each type's
/// `#[ts(export_to = "types.ts")]` attribute is just the filename, so every type
/// gets merged into a single `types.ts` under `out_dir`.
fn export_and_read() -> String {
    Workspace::export_all_to(types_ts_dir()).expect("export Workspace");
    SessionLifecycle::export_all_to(types_ts_dir()).expect("export SessionLifecycle");
    SessionMeta::export_all_to(types_ts_dir()).expect("export SessionMeta");
    TerminalEvent::export_all_to(types_ts_dir()).expect("export TerminalEvent");
    // CommandEvent is not a field/variant dependency of any other exported type, so it
    // needs its own explicit export call or it never lands in types.ts (spec §3.3).
    CommandEvent::export_all_to(types_ts_dir()).expect("export CommandEvent");
    fs::read_to_string(types_ts_path()).expect("read generated types.ts")
}

/// Whitespace- and quote-insensitive substring check so we assert structure, not
/// formatting. ts-rs quotes object-literal keys that need it (e.g. `"kind":
/// "atPrompt"`) while spec prose writes them unquoted (`kind: "atPrompt"`) — both are
/// the same TypeScript object-type key syntax, so quotes around bare identifier keys
/// are stripped before comparing.
fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let strip = |s: &str| s.split_whitespace().collect::<String>().replace('"', "");
    strip(haystack).contains(&strip(needle))
}

#[test]
fn generates_types_ts_at_shared_path() {
    let ts = export_and_read();
    assert!(!ts.is_empty(), "types.ts must not be empty");
    assert!(
        types_ts_path().exists(),
        "types.ts must exist at src/ipc/types.ts"
    );
    let _ = &ts;
}

#[test]
fn workspace_uses_camelcase_root_path() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "rootPath: string"),
        "Workspace.root_path must serialize as camelCase `rootPath`; got:\n{ts}"
    );
    assert!(
        !ts.contains("root_path"),
        "generated TS must not contain snake_case `root_path`"
    );
}

#[test]
fn workspace_exposes_roots_array() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "roots: Array<string>")
            || contains_normalized(&ts, "roots: string[]"),
        "Workspace.roots (Vec<String>) must be a string array; got:\n{ts}"
    );
}

#[test]
fn command_event_is_exported_camelcase_with_number_seq_and_ts() {
    let ts = export_and_read();
    assert!(
        ts.contains("export type CommandEvent"),
        "CommandEvent must be exported as a top-level TS type; got:\n{ts}"
    );
    for field in ["sessionId:", "exitCode:"] {
        assert!(
            contains_normalized(&ts, field),
            "CommandEvent must expose camelCase field `{field}`; got:\n{ts}"
        );
    }
    // seq/ts are i64 overridden to TS `number` (matches SessionMeta.createdAt), not bigint.
    assert!(
        contains_normalized(&ts, "seq: number"),
        "CommandEvent.seq must be overridden to TS `number`, not `bigint`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "ts: number"),
        "CommandEvent.ts must be overridden to TS `number`, not `bigint`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "kind: string"),
        "CommandEvent.kind must be a plain string (started|finished literals, spec §3.3); got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "exitCode: number | null")
            || contains_normalized(&ts, "exitCode: number|null"),
        "CommandEvent.exitCode (Option<u8>) must be nullable number; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "origin: string"),
        "CommandEvent.origin must be a plain string; got:\n{ts}"
    );
    assert!(
        !ts.contains("session_id") && !ts.contains("exit_code"),
        "no snake_case leakage in CommandEvent"
    );
}

#[test]
fn session_lifecycle_is_internally_tagged_camelcase() {
    let ts = export_and_read();
    for tag in ["atPrompt", "typing", "running", "exited"] {
        assert!(
            contains_normalized(&ts, &format!("kind: \"{tag}\"")),
            "SessionLifecycle must include internally-tagged variant kind:\"{tag}\"; got:\n{ts}"
        );
    }
    // Exited carries code:number|null and signal:string|null
    assert!(
        contains_normalized(&ts, "code: number | null")
            || contains_normalized(&ts, "code: number|null"),
        "Exited must carry nullable numeric code; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "signal: string | null")
            || contains_normalized(&ts, "signal: string|null"),
        "Exited must carry nullable string signal; got:\n{ts}"
    );
}

#[test]
fn terminal_event_is_adjacently_tagged_bytes_are_number_arrays() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "event: \"replay\""),
        "TerminalEvent must be adjacently tagged with event:\"replay\"; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "event: \"output\""),
        "TerminalEvent must be adjacently tagged with event:\"output\"; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "data:"),
        "TerminalEvent variants must nest their payload under `data`; got:\n{ts}"
    );
    // Vec<u8> must be a number array (ts-rs emits Array<number>).
    assert!(
        contains_normalized(&ts, "content: Array<number>")
            || contains_normalized(&ts, "content: number[]"),
        "Replay.content (Vec<u8>) must be a number array; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "bytes: Array<number>")
            || contains_normalized(&ts, "bytes: number[]"),
        "Output.bytes (Vec<u8>) must be a number array; got:\n{ts}"
    );
}

#[test]
fn session_meta_fields_are_camelcase() {
    let ts = export_and_read();
    for field in [
        "workspaceId:",
        "waitingForInput:",
        "isActive:",
        "createdAt:",
    ] {
        assert!(
            contains_normalized(&ts, field),
            "SessionMeta must expose camelCase field `{field}`; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("workspace_id"),
        "no snake_case leakage in SessionMeta"
    );
}
