//! RuleSet markdown FILE layer (spec §7, D4): rules markdown files are the source of truth; the
//! DB only stores `md_path` + a sha256 `md_hash`. This module owns ALL of orchd's file I/O — the
//! ONLY file family orchd ever touches (D4's narrow, deliberate exception to "orchd gets its own
//! general file API in S9" — architecture.md T21). Two primitives: [`write_atomic`] (create parent
//! dirs, atomic tmp+rename write, returns the sha256 hex of the content just written) and
//! [`read_state`] (read-fresh-every-time state classification: `Ok` / `Missing` /
//! `ExternallyModified` — spec §7 `GetRuleSet` semantics). [`sha256_hex`] is the single hashing
//! implementation both of them (and `persistence::Db::acknowledge_rule_file`, via re-use) build
//! on, so "the hash of a ruleset file" is computed exactly one way anywhere in this crate —
//! `boot::ensure_global_ruleset` is re-seated onto it too (T8), instead of keeping its own
//! duplicate `Sha256::digest` call.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use bpa_orchd_proto::RuleFileState;

/// sha256 hex digest of `content`'s UTF-8 bytes (spec §7: "`md_hash = sha256(content)` hex").
pub fn sha256_hex(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
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
/// Not yet called from this crate's production code: `OrchdRequest::GetRuleSet`'s dispatch
/// handler (a later task, spec §5/§6) is the intended production caller, combining this with
/// `persistence::Db::get_ruleset`'s DB-row half into the wire `RuleSetView`. Exercised directly
/// by this module's own tests (and `persistence::ruleset_tests`) until then.
#[allow(dead_code)]
pub fn read_state(path: &Path, stored_hash: &str) -> (Option<String>, RuleFileState) {
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
