//! RuleSet markdown FILE layer (spec §7, D4): rules markdown files are the source of truth; the
//! DB only stores `md_path` + a sha256 `md_hash`. This module owns ALL of orchd's file I/O —
//! originally the rules-file family alone (D4's narrow, deliberate exception to "orchd gets its
//! own general file API in S9" — architecture.md T21), and since SCN-054 the per-project doc
//! files too (docs are "rules.md × N named files" and reuse this exact layer rather than growing
//! a parallel one). Four primitives: [`write_atomic`] (create parent dirs, atomic tmp+rename
//! write, returns the sha256 hex of the content just written), [`read_state`]
//! (read-fresh-every-time state classification: `Ok` / `Missing` / `ExternallyModified` — spec §7
//! `GetRuleSet` semantics, reused verbatim by SCN-054's `GetDoc`), [`modified_at_ms`] (the doc
//! list's honest last-modified, SCN-054) and [`remove_if_exists`] (`DeleteDoc`'s file removal).
//! [`sha256_hex`] is the single hashing implementation the write/read pair (and
//! `persistence::Db::acknowledge_rule_file`/`acknowledge_doc_file`, via re-use) build on, so "the
//! hash of a markdown file" is computed exactly one way anywhere in this crate —
//! `boot::ensure_global_ruleset` is re-seated onto it too (T8), instead of keeping its own
//! duplicate `Sha256::digest` call.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use bpa_orchd_proto::RuleFileState;

/// sha256 hex digest of `content`'s UTF-8 bytes (spec §7: "`md_hash = sha256(content)` hex").
pub fn sha256_hex(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// BL-77: hard ceiling on a single ruleset/doc/skill markdown read. These files are owner-authored
/// markdown (rules, docs, SKILL.md) — naturally small. A hostile or corrupted file pointed at via
/// `md_path` (which IS symlink-escape-guarded, but not size-guarded) would otherwise be read to
/// completion into a `String` → unbounded memory + latency (a DoS vector against orchd). Mirrors
/// `fs_explorer::read_file_preview`'s 1 MiB stat-before-read cap. `read_state` /
/// `skills::compute_file_state` fold an oversized file into their existing `Missing` honest-
/// degradation state (the file cannot be read safely right now); `skills::add_skill` surfaces it
/// as a typed `Validation`.
pub const MAX_MD_READ_BYTES: u64 = 1024 * 1024;

/// True when `path`'s stat'd size exceeds [`MAX_MD_READ_BYTES`]. A stat failure (missing file, a
/// directory at `path`, permission denied) returns `false` so the caller's normal read path still
/// runs and surfaces the right state (`Missing`/`Validation`) for THAT failure — this gate only
/// answers the oversized question.
pub fn exceeds_read_cap(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.len() > MAX_MD_READ_BYTES,
        Err(_) => false,
    }
}

/// `<path>` with a `.tmp` suffix appended to its final component — [`write_atomic`]'s staging
/// file.
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Atomically write `content` to `path` (spec §7: "create parent dirs, write file atomically
/// (tmp+rename)"). Writes to `<path>.tmp` first, then [`std::fs::rename`]s it over `path` — on
/// every platform this crate targets, `rename(2)` within the same filesystem is atomic, so a
/// concurrent reader can never observe a partially-written file, and a crash between the write and
/// the rename leaves the ORIGINAL file (if any) untouched plus, at worst, an orphaned `.tmp` —
/// never a truncated/corrupt `path`. Returns the sha256 hex of `content` (spec §7); the caller
/// stores this in the DB row's `md_hash`.
pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = tmp_path_for(path);
    std::fs::write(&tmp_path, content.as_bytes())?;
    std::fs::rename(&tmp_path, path)?;
    Ok(sha256_hex(content))
}

/// Read `path` fresh (spec §7: "`GetRuleSet`: read file fresh each time") and classify it against
/// `stored_hash` (the DB row's `md_hash`):
/// - missing ⇒ `(None, Missing)`
/// - present and its sha256 matches `stored_hash` ⇒ `(Some(content), Ok)`
/// - present but its sha256 does NOT match (hand-edited or replaced outside orchd) ⇒
///   `(Some(content), ExternallyModified)`
///
/// Any OTHER read failure (permission denied, non-UTF8 content, a directory at `path`, …) is
/// folded into `Missing` too — orchd has nothing more specific and actionable to tell the owner
/// than "the file this row points at cannot be read right now" (mirrors this crate's general
/// honest-degradation stance, e.g. `persistence::open_db_degrading`'s in-memory fallback). File
/// content is NEVER logged by this function or its caller (spec §5 no-secrets discipline).
///
/// Called from production code in two places: `socket_server::build_ruleset_view` (spec §5/§6's
/// `GetRuleSet` dispatch handler, pairing this with `persistence::Db::get_ruleset`'s DB-row half
/// into the wire `RuleSetView`) and `export::read_live_md_content` (spec §8's export bundling,
/// which only wants the `Option<String>` content half). Also exercised directly by this module's
/// own tests (and `persistence::ruleset_tests`).
pub fn read_state(path: &Path, stored_hash: &str) -> (Option<String>, RuleFileState) {
    // BL-77: an oversized file folds into `Missing` (the existing honest-degradation "cannot be
    // read right now" state) instead of being buffered into memory wholesale.
    if exceeds_read_cap(path) {
        return (None, RuleFileState::Missing);
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            if sha256_hex(&content) == stored_hash {
                (Some(content), RuleFileState::Ok)
            } else {
                (Some(content), RuleFileState::ExternallyModified)
            }
        }
        Err(_) => (None, RuleFileState::Missing),
    }
}

/// The file's mtime as unix-ms, read fresh (SCN-054: the doc list's "last-modified" column is
/// files-as-truth — an agent's external edit must move it without any daemon write). `None` when
/// the file is missing/unreadable OR its mtime predates the unix epoch (a nonsensical clock —
/// reported as "unknown" rather than a fabricated negative timestamp); the caller
/// (`socket_server`'s `ListDocs` arm) falls back to the DB row's `updated_at`, mirroring this
/// crate's honest-degradation stance ([`read_state`]'s fold-to-`Missing` doc above).
pub fn modified_at_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since_epoch.as_millis() as i64)
}

/// Remove the file at `path`, treating "already gone" as success (SCN-054 `DeleteDoc`: deleting
/// a doc whose file was lost externally must still delete the row — the file's absence is the
/// very state being cleaned up, not an error). Every OTHER failure (permission denied, a
/// directory at `path`, …) is surfaced so `Db::delete_doc` can abort BEFORE the row is deleted —
/// never leaving an orphaned on-disk file the UI no longer lists.
pub fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_creates_parent_dirs_and_returns_the_exact_content_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deep/rules.md");

        let hash = write_atomic(&path, "# hi\n").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# hi\n");
        assert_eq!(hash, sha256_hex("# hi\n"));
        // sha256("# hi\n") — pinned so a hashing-implementation regression (e.g. hashing the
        // wrong bytes, or double-hashing) fails loudly instead of just "matches itself".
        assert_eq!(
            hash,
            "045d2d07c2db3b9e6cef022457ee89434045a508c2dadccf9abe182ad633c273"
        );
    }

    #[test]
    fn write_atomic_leaves_no_tmp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.md");

        write_atomic(&path, "content").unwrap();

        assert!(
            !tmp_path_for(&path).exists(),
            "a .tmp staging file must not survive a successful write_atomic"
        );
    }

    #[test]
    fn write_atomic_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.md");

        write_atomic(&path, "v1").unwrap();
        let hash2 = write_atomic(&path, "v2").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2");
        assert_eq!(hash2, sha256_hex("v2"));
    }

    #[test]
    fn read_state_missing_file_is_missing_with_no_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.md");

        let (content, state) = read_state(&path, "irrelevant-hash");

        assert_eq!(content, None);
        assert_eq!(state, RuleFileState::Missing);
    }

    // BL-77: an oversized markdown file must NOT be buffered into memory — it folds into the
    // existing `Missing` honest-degradation state (the file cannot be read safely right now),
    // never reaching `read_to_string`.
    #[test]
    fn read_state_oversized_file_folds_to_missing_without_reading_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.md");
        std::fs::write(&path, "a".repeat(MAX_MD_READ_BYTES as usize + 1024)).unwrap();

        let (content, state) = read_state(&path, "irrelevant-hash");

        assert_eq!(
            content, None,
            "an oversized file must not be read into memory"
        );
        assert_eq!(state, RuleFileState::Missing);
    }

    #[test]
    fn read_state_matching_hash_is_ok_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.md");
        let hash = write_atomic(&path, "hello").unwrap();

        let (content, state) = read_state(&path, &hash);

        assert_eq!(content, Some("hello".to_string()));
        assert_eq!(state, RuleFileState::Ok);
    }

    #[test]
    fn read_state_mismatched_hash_is_externally_modified_with_the_new_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.md");
        write_atomic(&path, "hello").unwrap();
        let stale_hash = sha256_hex("hello");

        // Someone edits the file directly on disk, bypassing write_atomic entirely.
        std::fs::write(&path, "someone edited this").unwrap();

        let (content, state) = read_state(&path, &stale_hash);

        assert_eq!(content, Some("someone edited this".to_string()));
        assert_eq!(state, RuleFileState::ExternallyModified);
    }

    // ---- SCN-054 doc-file primitives ----

    #[test]
    fn modified_at_ms_of_a_written_file_is_a_recent_unix_ms_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        write_atomic(&path, "# notes\n").unwrap();

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mtime = modified_at_ms(&path).expect("a just-written file must have a readable mtime");

        // Within a minute either side of "now" — proves unix-MILLISECOND scale (a seconds-scale
        // regression would be ~1000× too small and fail loudly here).
        assert!(
            (mtime - before).abs() < 60_000,
            "mtime {mtime} not within 60s of now {before}"
        );
    }

    #[test]
    fn modified_at_ms_of_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(modified_at_ms(&dir.path().join("nope.md")), None);
    }

    #[test]
    fn remove_if_exists_removes_a_present_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doomed.md");
        write_atomic(&path, "bye").unwrap();

        remove_if_exists(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn remove_if_exists_is_ok_when_the_file_is_already_gone() {
        // SCN-054: deleting a "file lost" doc must still succeed — absence is the cleaned-up
        // state, not an error.
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_if_exists(&dir.path().join("never-existed.md")).is_ok());
    }

    #[test]
    fn remove_if_exists_surfaces_a_non_missing_failure() {
        // A directory at `path` cannot be removed by remove_file — this must surface as an error
        // (so `Db::delete_doc` aborts before deleting the row), NOT be swallowed like NotFound.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a-directory.md");
        std::fs::create_dir(&path).unwrap();

        assert!(remove_if_exists(&path).is_err());
    }

    #[test]
    fn read_state_unreadable_path_is_missing() {
        // A directory at `path` can never be read as file content — folds into Missing, same as
        // a genuinely absent file (module doc: "any OTHER read failure ... is folded into
        // Missing too").
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a-directory.md");
        std::fs::create_dir(&path).unwrap();

        let (content, state) = read_state(&path, "irrelevant-hash");

        assert_eq!(content, None);
        assert_eq!(state, RuleFileState::Missing);
    }
}
