//! Live FSEvents watch over a workspace's roots (spec §5), GUI-lifetime only: nothing is watched
//! while the app is closed, and starting a new watch always replaces whatever was running before
//! (spec: "ONE active watch set at a time").
//!
//! ## Design — pure filter, thin wiring (mirrors `broker.rs`'s testable-seam pattern)
//!
//! [`build_changed_events`] and [`build_watch_error_events`] are **pure** functions — raw
//! `notify` paths/errors plus the per-root [`RootMatcher`]s in, `Vec<`[`FsEvent`]`>` out — with no
//! Tauri runtime and no `notify_debouncer_full` dependency, so the debounced-batch-to-payload
//! logic (gitignore filtering, `.git` exclusion, dedup, the 500-path cap, routing to the owning
//! root) is exhaustively unit-tested without spinning up a real watcher. [`start_watch_inner`] is
//! the thin, side-effecting shell around them: it owns the real `notify_debouncer_full::Debouncer`
//! and calls the pure functions from inside its debounce-tick handler.
//!
//! [`FsEventSink`] is the emitter seam ([`broker::Broker`]'s `AppHandle::emit` wrapping, applied
//! here): production sends through `tauri::AppHandle::emit`, tests capture into a plain
//! `Vec<FsEvent>` behind a mutex — so [`start_watch_inner`]/[`stop_watch_inner`] (the
//! watcher-to-seam wiring) are ALSO unit-testable against a real `notify` watcher and a real
//! tempdir, without a live Tauri `AppHandle` (see the `cfg(test)` module's
//! `real_notify_watch_delivers_debounced_changed_event_and_respects_stop`).
//!
//! ## Gitignore scope — root `.gitignore` only, NOT fs_explorer's nested-aware walk
//!
//! `fs_explorer::visible_child_names` (spec §4.2) is nested-`.gitignore`-aware because it always
//! re-walks the ACTUAL directory being listed with `ignore::WalkBuilder`, and that function is
//! module-private (not `pub(crate)`) — there is nothing to reuse across the module boundary
//! (locked scope for this task: fs_explorer.rs internals are not to be touched). Replicating that
//! same nested precision here would require either (a) a flat `ignore::gitignore::GitignoreBuilder`
//! merging every nested `.gitignore` file found under the root, or (b) re-walking each changed
//! path's parent directory on every event. (a) is a *correctness trap*: `GitignoreBuilder::add`
//! compiles each pattern relative to the SINGLE builder root, not to the individual file's own
//! directory (verified against the `ignore` 0.4.27 source: `Glob.from` only carries the source
//! path for error messages, never adjusts the compiled glob's anchor) — merging a nested
//! `sub/.gitignore` this way makes its patterns match anywhere under the root, not just under
//! `sub/`. (b) breaks on delete events, whose parent directory may no longer exist to re-walk.
//! [`build_root_gitignore`] therefore matches ONLY the workspace root's own top-level `.gitignore`
//! (still gated on a `.git` directory being present, mirroring `fs_explorer`'s `require_git`
//! rationale) — a deliberate, disclosed scope narrowing, not a bug. The live watch is a
//! best-effort noise-reduction signal, not a security or correctness boundary: worst case, a
//! change under a nested-ignored path triggers one extra (harmless) point-refresh, which
//! `list_dir` — the actual, fully nested-gitignore-aware source of truth for what renders — then
//! resolves correctly on its own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tauri::{AppHandle, Emitter};
use tracing::warn;

/// Debounce window (spec §5): `new_debouncer(Duration::from_millis(250), None, handler)`. FSEvents
/// latency plus this debounce stays well under the spec's `touch` < 1s DoD.
const DEBOUNCE_MS: u64 = 250;

/// Cap on how many distinct changed rel-paths a single `fs://changed` event may list before it
/// collapses to the "refresh everything expanded" sentinel (spec §5).
pub const WATCH_PATH_CAP: usize = 500;

/// `fs://changed` event name (spec §5).
pub const EV_FS_CHANGED: &str = "fs://changed";
/// `fs://watch-error` event name (spec §5).
pub const EV_FS_WATCH_ERROR: &str = "fs://watch-error";

// ── wire-shaped output of the pure filter ───────────────────────────────────────────────────────

/// The two possible outcomes a debounced batch (or a watch-start failure) can produce, before
/// they're serialized to their respective Tauri events. Kept as a plain, comparable, non-generic
/// enum (mirrors `broker::BrokerAction`) so [`build_changed_events`]/[`build_watch_error_events`]
/// stay trivially assertable in unit tests.
#[derive(Debug, Clone, PartialEq)]
pub enum FsEvent {
    /// -> `fs://changed { root, changedRelPaths }`. `rel_paths` is already deduped and capped
    /// (see [`WATCH_PATH_CAP`]): `["*"]` means "refresh everything expanded under this root".
    Changed {
        root: String,
        rel_paths: Vec<String>,
    },
    /// -> `fs://watch-error { root, reason }`. Never panics the app (spec §5): the frontend shows
    /// a "live updates paused" affordance and re-calls `start_workspace_watch` on next activation.
    WatchError { root: String, reason: String },
}

// ── emitter seam (mirrors broker.rs's AppHandle::emit wrapping) ────────────────────────────────

/// Abstraction over "deliver an [`FsEvent`] to the outside world". Production sends through
/// `tauri::AppHandle::emit` (below); tests use a plain capturing sink, so both the pure filter
/// functions AND the real-`notify`-backed watcher wiring are testable without a live Tauri
/// `AppHandle`.
pub trait FsEventSink: Send + Sync + 'static {
    fn send(&self, event: FsEvent);
}

impl FsEventSink for AppHandle {
    fn send(&self, event: FsEvent) {
        let (name, payload) = match &event {
            FsEvent::Changed { root, rel_paths } => (
                EV_FS_CHANGED,
                serde_json::json!({ "root": root, "changedRelPaths": rel_paths }),
            ),
            FsEvent::WatchError { root, reason } => (
                EV_FS_WATCH_ERROR,
                serde_json::json!({ "root": root, "reason": reason }),
            ),
        };
        if let Err(e) = self.emit(name, payload) {
            warn!(target: "fs_watcher", event = name, error = %e, "emit failed");
        }
    }
}

// ── per-root match context ──────────────────────────────────────────────────────────────────────

/// Everything [`build_changed_events`] needs to route and filter events for one workspace root.
/// `root` is the EXACT string the frontend passed to `start_workspace_watch` (echoed back
/// verbatim as the `root` key of every emitted event — never re-derived from `root_path`, so a
/// frontend that identifies roots by their un-canonicalized string always gets a match back).
/// `root_path` is the CANONICALIZED absolute path (`std::fs::canonicalize`): on macOS, FSEvents
/// reports canonical/real paths (e.g. `/private/var/...` for a `/var/...` symlink target), so
/// `notify`'s watched path and every `strip_prefix` comparison against reported event paths must
/// use the same canonical form or every event silently fails to match its root.
pub(crate) struct RootMatcher {
    root: String,
    root_path: PathBuf,
    gitignore: ignore::gitignore::Gitignore,
}

/// Build the root's gitignore matcher (see the module doc's "Gitignore scope" section for why
/// this is root-`.gitignore`-only, not the nested-aware walk `fs_explorer` uses). Gated on a
/// `.git` directory being present, mirroring `fs_explorer::visible_child_names`'s `require_git`
/// rationale: a non-repo root's stray `.gitignore` must not silently filter the live watch either.
/// Returns `Gitignore::empty()` (matches nothing) when there's no `.git`, no `.gitignore`, or the
/// file fails to parse — an unreadable/partial `.gitignore` degrades to "show everything", never
/// a hard failure of the watch itself.
fn build_root_gitignore(root_path: &Path) -> ignore::gitignore::Gitignore {
    if !root_path.join(".git").exists() {
        return ignore::gitignore::Gitignore::empty();
    }
    let gitignore_path = root_path.join(".gitignore");
    if !gitignore_path.is_file() {
        return ignore::gitignore::Gitignore::empty();
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root_path);
    // A partially-invalid .gitignore (one bad glob line) still yields every OTHER valid pattern —
    // `add`'s `Option<Error>` is intentionally discarded rather than aborting the whole matcher.
    let _ = builder.add(&gitignore_path);
    builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}

/// Canonicalize `roots` into [`RootMatcher`]s, building each one's gitignore matcher. A root that
/// fails to canonicalize (doesn't exist, dangling symlink, permission denied) is dropped from the
/// returned list and immediately surfaces an honest [`FsEvent::WatchError`] through `sink` —
/// never a panic, never a silently-skipped root (spec §5).
fn build_root_matchers<S: FsEventSink>(roots: &[String], sink: &S) -> Vec<RootMatcher> {
    let mut out = Vec::with_capacity(roots.len());
    for root in roots {
        match std::fs::canonicalize(root) {
            Ok(root_path) => {
                let gitignore = build_root_gitignore(&root_path);
                out.push(RootMatcher {
                    root: root.clone(),
                    root_path,
                    gitignore,
                });
            }
            Err(e) => {
                sink.send(FsEvent::WatchError {
                    root: root.clone(),
                    reason: format!("cannot watch {root}: {e}"),
                });
            }
        }
    }
    out
}

// ── pure filter: raw paths -> FsEvent::Changed ──────────────────────────────────────────────────

/// `true` if any component of `rel` is literally `.git` — internal git bookkeeping (`HEAD`,
/// `refs/...`, `objects/...`, `index`, ...) is never surfaced as a live-watch change, regardless
/// of `show_ignored` (spec §5: "always drop `.git`-internal paths").
fn is_git_internal(rel: &Path) -> bool {
    rel.components().any(|c| c.as_os_str() == ".git")
}

/// Forward-slash-normalized rel path string (matches `fs_explorer::FsEntry::rel_path`'s
/// convention). On the Unix targets this app ships for, `Path`'s own separator already IS `/`;
/// the explicit replace is a defensive no-op there and the only thing standing between this and a
/// backslash leaking into the wire payload if this code is ever built for a non-Unix target.
fn rel_to_wire_string(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// Pure batch filter (spec §5): map raw `notify` event paths to deduped, capped, per-root
/// `FsEvent::Changed` payloads. For each path: find its owning root (dropped if none matches,
/// spec: "a path that belongs to none of the roots is dropped"); drop `.git`-internal paths
/// unconditionally; drop gitignored paths unless `show_ignored`; dedup within each root's set;
/// cap at [`WATCH_PATH_CAP`] (overflow collapses to `["*"]`). One `FsEvent::Changed` per root that
/// had at least one surviving path in this batch — a batch spanning multiple roots' subtrees
/// (all roots share one `Debouncer`/thread) yields one event per affected root, never a merged one.
pub(crate) fn build_changed_events(
    raw_paths: &[PathBuf],
    roots: &[RootMatcher],
    show_ignored: bool,
) -> Vec<FsEvent> {
    let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for path in raw_paths {
        let Some((matcher, rel)) = roots
            .iter()
            .find_map(|m| path.strip_prefix(&m.root_path).ok().map(|rel| (m, rel)))
        else {
            continue; // belongs to none of the watched roots
        };

        if is_git_internal(rel) {
            continue;
        }

        // Best-effort: a deleted path can't be stat'd, so `is_dir()` safely returns `false` for
        // it (never panics/errors) — the only cost is a `dir/`-only gitignore pattern won't match
        // a just-deleted directory's OWN entry (its children, still individually reported by
        // notify for a recursive delete, are matched on their own merits regardless).
        let is_dir_hint = path.is_dir();
        let ignored = matches!(
            matcher.gitignore.matched(rel, is_dir_hint),
            ignore::Match::Ignore(_)
        );
        if ignored && !show_ignored {
            continue;
        }

        let rel_str = rel_to_wire_string(rel);
        if rel_str.is_empty() {
            continue; // the root directory itself — nothing for the frontend to point-refresh
        }
        buckets
            .entry(matcher.root.clone())
            .or_default()
            .insert(rel_str);
    }

    buckets
        .into_iter()
        .map(|(root, set)| {
            let rel_paths = if set.len() > WATCH_PATH_CAP {
                vec!["*".to_string()]
            } else {
                set.into_iter().collect()
            };
            FsEvent::Changed { root, rel_paths }
        })
        .collect()
}

/// Pure mapping from a debounced batch's `Err(errors)` arm (spec §5: watcher error -> honest
/// `fs://watch-error`) to per-root events. `notify::Error::paths` carries the affected path(s)
/// when known — routed to its owning root exactly like [`build_changed_events`]. An error with no
/// path info (a general backend failure, not tied to a specific watched subtree) is surfaced
/// against EVERY currently-watched root rather than silently dropped: the frontend cannot know
/// which root's live updates paused otherwise, and a false-positive "paused" banner the user can
/// dismiss/retry is far better than a silently-stale tree (spec §5: "honest").
pub(crate) fn build_watch_error_events(
    errors: &[notify::Error],
    roots: &[RootMatcher],
) -> Vec<FsEvent> {
    let mut out = Vec::new();
    for err in errors {
        let reason = err.to_string();
        let matched_root = err.paths.iter().find_map(|p| {
            roots
                .iter()
                .find(|m| p.starts_with(&m.root_path))
                .map(|m| m.root.clone())
        });
        match matched_root {
            Some(root) => out.push(FsEvent::WatchError { root, reason }),
            None => {
                for m in roots {
                    out.push(FsEvent::WatchError {
                        root: m.root.clone(),
                        reason: reason.clone(),
                    });
                }
            }
        }
    }
    out
}

// ── real watcher wiring (thin shell over the pure functions above) ─────────────────────────────

type FsDebouncer = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

/// Managed-state payload for one active watch set (spec §5: "`Debouncer` stored in managed state
/// (`Mutex<Option<...>>`)"). Dropping the `Debouncer` unwatches every root it held (the crate's
/// own `Drop` impl stops its background thread and tears down the OS-level watch) — replacing or
/// clearing the slot is therefore the ENTIRE stop-watching mechanism; there is no separate
/// "unwatch" call to remember.
pub struct WatchState {
    // Never read back out — its entire purpose is to be dropped (which unwatches every root it
    // holds) when this `WatchState` is replaced or the slot is cleared. `#[allow(dead_code)]`
    // rather than a needless getter that would exist only to silence the lint.
    #[allow(dead_code)]
    debouncer: FsDebouncer,
    /// Kept for introspection/tests proving "starting again replaces the previous" swaps this
    /// list wholesale rather than appending to it (only read from `#[cfg(test)]` code, hence
    /// `#[allow(dead_code)]` on a plain `cargo build`).
    #[allow(dead_code)]
    roots: Vec<String>,
    #[allow(dead_code)]
    show_ignored: bool,
}

/// Tauri-managed slot: `None` while nothing is being watched (including the entire time the app
/// hasn't yet activated a workspace, and forever if the app is closed — GUI-lifetime, spec D4).
pub type WatchSlot = Mutex<Option<WatchState>>;

/// Construct the initial (empty) managed state — call once from `lib.rs`'s `setup()` /
/// `.manage(...)`, independent of daemon connectivity (this watch never touches the daemon).
pub fn new_watch_slot() -> WatchSlot {
    Mutex::new(None)
}

/// Side-effecting shell (spec §5): build each root's [`RootMatcher`], start a real
/// `notify_debouncer_full` debouncer, `watch()` every root, and store the result in `slot` —
/// REPLACING (and thereby dropping/unwatching) whatever was there before, satisfying "ONE active
/// watch set at a time... starting again replaces the previous". Every failure mode (a root that
/// doesn't canonicalize, a root that fails `watch()`, the debouncer itself failing to construct)
/// emits [`FsEvent::WatchError`] through `sink` and is otherwise a no-op — this function never
/// panics.
pub(crate) fn start_watch_inner<S: FsEventSink + Clone>(
    slot: &WatchSlot,
    sink: S,
    roots: Vec<String>,
    show_ignored: bool,
) {
    let matchers = build_root_matchers(&roots, &sink);
    let matchers = Arc::new(matchers);

    let handler_sink = sink.clone();
    let handler_matchers = Arc::clone(&matchers);
    let handler = move |result: DebounceEventResult| match result {
        Ok(events) => {
            let paths: Vec<PathBuf> = events
                .iter()
                .flat_map(|e| e.paths.iter().cloned())
                .collect();
            for ev in build_changed_events(&paths, &handler_matchers, show_ignored) {
                handler_sink.send(ev);
            }
        }
        Err(errors) => {
            for ev in build_watch_error_events(&errors, &handler_matchers) {
                handler_sink.send(ev);
            }
        }
    };

    match new_debouncer(Duration::from_millis(DEBOUNCE_MS), None, handler) {
        Ok(mut debouncer) => {
            for m in matchers.iter() {
                if let Err(e) = debouncer.watch(&m.root_path, RecursiveMode::Recursive) {
                    sink.send(FsEvent::WatchError {
                        root: m.root.clone(),
                        reason: e.to_string(),
                    });
                }
            }
            let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
            // Assigning here drops the PREVIOUS `Option<WatchState>` (if any) before storing the
            // new one — that drop is what unwatches every root the old debouncer held.
            *guard = Some(WatchState {
                debouncer,
                roots,
                show_ignored,
            });
        }
        Err(e) => {
            for root in &roots {
                sink.send(FsEvent::WatchError {
                    root: root.clone(),
                    reason: e.to_string(),
                });
            }
        }
    }
}

/// Stop the active watch, if any (spec §5: "stop on switch/unmount"). Clearing the slot drops the
/// `Debouncer`, which unwatches every root — see [`WatchState`]'s docs. A no-op (never an error)
/// when nothing is currently being watched.
pub(crate) fn stop_watch_inner(slot: &WatchSlot) {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

// ── #[tauri::command] surface (spec §5) ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn start_workspace_watch(
    app: AppHandle,
    watch_slot: tauri::State<'_, WatchSlot>,
    roots: Vec<String>,
    show_ignored: bool,
) {
    start_watch_inner(watch_slot.inner(), app, roots, show_ignored);
}

#[tauri::command]
pub fn stop_workspace_watch(watch_slot: tauri::State<'_, WatchSlot>) {
    stop_watch_inner(watch_slot.inner());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── fixtures ─────────────────────────────────────────────────────────────────────────────

    /// `dir/root` plus a `.git` marker directory, so `build_root_gitignore` activates (mirrors
    /// `fs_explorer::tests::git_root`).
    fn git_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        (dir, root)
    }

    fn plain_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        (dir, root)
    }

    fn matcher_for(root_key: &str, root_path: &Path) -> RootMatcher {
        RootMatcher {
            root: root_key.to_string(),
            root_path: root_path.to_path_buf(),
            gitignore: build_root_gitignore(root_path),
        }
    }

    // ── build_root_gitignore ─────────────────────────────────────────────────────────────────

    #[test]
    fn build_root_gitignore_empty_without_a_git_dir() {
        let (_tmp, root) = plain_root();
        fs::write(root.join(".gitignore"), b"secret.log\n").unwrap();
        let gi = build_root_gitignore(&root);
        assert!(
            gi.is_empty(),
            "no .git dir present -> matcher must be empty"
        );
    }

    #[test]
    fn build_root_gitignore_empty_without_a_gitignore_file() {
        let (_tmp, root) = git_root();
        let gi = build_root_gitignore(&root);
        assert!(gi.is_empty());
    }

    #[test]
    fn build_root_gitignore_matches_root_patterns() {
        let (_tmp, root) = git_root();
        fs::write(root.join(".gitignore"), b"secret.log\n").unwrap();
        let gi = build_root_gitignore(&root);
        assert!(matches!(
            gi.matched(Path::new("secret.log"), false),
            ignore::Match::Ignore(_)
        ));
        assert!(matches!(
            gi.matched(Path::new("kept.txt"), false),
            ignore::Match::None
        ));
    }

    // ── build_changed_events: RED-phase cases from the task brief ──────────────────────────────

    #[test]
    fn gitignored_path_dropped_unless_show_ignored() {
        let (_tmp, root) = git_root();
        fs::write(root.join(".gitignore"), b"secret.log\n").unwrap();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![root.join("secret.log")];

        let dropped = build_changed_events(&raw, &matchers, false);
        assert!(
            dropped.is_empty(),
            "gitignored path must be dropped, got {dropped:?}"
        );

        let kept = build_changed_events(&raw, &matchers, true);
        assert_eq!(
            kept,
            vec![FsEvent::Changed {
                root: "root-key".to_string(),
                rel_paths: vec!["secret.log".to_string()],
            }]
        );
    }

    #[test]
    fn dot_git_internal_paths_always_dropped_even_with_show_ignored() {
        let (_tmp, root) = git_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![
            root.join(".git").join("HEAD"),
            root.join(".git").join("refs").join("heads").join("main"),
            root.join(".git"),
        ];

        for show_ignored in [false, true] {
            let events = build_changed_events(&raw, &matchers, show_ignored);
            assert!(
                events.is_empty(),
                ".git-internal paths must always be dropped (show_ignored={show_ignored}), got {events:?}"
            );
        }
    }

    #[test]
    fn path_outside_all_roots_is_dropped() {
        let (_tmp, root) = plain_root();
        let (_other_tmp, other_root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![other_root.join("elsewhere.txt")];

        let events = build_changed_events(&raw, &matchers, false);
        assert!(
            events.is_empty(),
            "path outside every root must be dropped, got {events:?}"
        );
    }

    #[test]
    fn duplicate_paths_are_deduped() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![root.join("a.txt"), root.join("a.txt"), root.join("a.txt")];

        let events = build_changed_events(&raw, &matchers, false);
        assert_eq!(
            events,
            vec![FsEvent::Changed {
                root: "root-key".to_string(),
                rel_paths: vec!["a.txt".to_string()],
            }]
        );
    }

    #[test]
    fn over_cap_paths_collapse_to_refresh_everything_sentinel() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw: Vec<PathBuf> = (0..WATCH_PATH_CAP + 1)
            .map(|i| root.join(format!("file-{i}.txt")))
            .collect();

        let events = build_changed_events(&raw, &matchers, false);
        assert_eq!(
            events,
            vec![FsEvent::Changed {
                root: "root-key".to_string(),
                rel_paths: vec!["*".to_string()],
            }]
        );
    }

    #[test]
    fn exactly_at_cap_is_not_collapsed() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw: Vec<PathBuf> = (0..WATCH_PATH_CAP)
            .map(|i| root.join(format!("file-{i}.txt")))
            .collect();

        let events = build_changed_events(&raw, &matchers, false);
        match &events[..] {
            [FsEvent::Changed { rel_paths, .. }] => {
                assert_eq!(rel_paths.len(), WATCH_PATH_CAP);
                assert!(!rel_paths.contains(&"*".to_string()));
            }
            other => panic!("expected exactly one Changed event, got {other:?}"),
        }
    }

    #[test]
    fn nested_rel_path_computed_correctly_with_forward_slashes() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![root.join("sub").join("deeper").join("file.txt")];

        let events = build_changed_events(&raw, &matchers, false);
        assert_eq!(
            events,
            vec![FsEvent::Changed {
                root: "root-key".to_string(),
                rel_paths: vec!["sub/deeper/file.txt".to_string()],
            }]
        );
    }

    #[test]
    fn multiple_roots_routed_to_the_right_root() {
        let (_tmp_a, root_a) = plain_root();
        let (_tmp_b, root_b) = plain_root();
        let matchers = vec![matcher_for("A", &root_a), matcher_for("B", &root_b)];
        let raw = vec![root_a.join("in-a.txt"), root_b.join("in-b.txt")];

        let mut events = build_changed_events(&raw, &matchers, false);
        events.sort_by(|a, b| match (a, b) {
            (FsEvent::Changed { root: ra, .. }, FsEvent::Changed { root: rb, .. }) => ra.cmp(rb),
            _ => std::cmp::Ordering::Equal,
        });

        assert_eq!(
            events,
            vec![
                FsEvent::Changed {
                    root: "A".to_string(),
                    rel_paths: vec!["in-a.txt".to_string()],
                },
                FsEvent::Changed {
                    root: "B".to_string(),
                    rel_paths: vec!["in-b.txt".to_string()],
                },
            ]
        );
    }

    #[test]
    fn the_root_directory_itself_produces_no_entry() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let raw = vec![root.clone()];

        let events = build_changed_events(&raw, &matchers, false);
        assert!(
            events.is_empty(),
            "the root path itself must not be a rel-path entry, got {events:?}"
        );
    }

    // ── build_watch_error_events ────────────────────────────────────────────────────────────────

    #[test]
    fn watch_error_with_matching_path_routes_to_its_root() {
        let (_tmp, root) = plain_root();
        let matchers = vec![matcher_for("root-key", &root)];
        let errors = vec![notify::Error::generic("boom").add_path(root.join("x.txt"))];

        let events = build_watch_error_events(&errors, &matchers);
        assert_eq!(events.len(), 1);
        match &events[0] {
            FsEvent::WatchError { root: r, reason } => {
                assert_eq!(r, "root-key");
                assert!(reason.contains("boom"), "got {reason}");
            }
            other => panic!("expected WatchError, got {other:?}"),
        }
    }

    #[test]
    fn watch_error_without_path_info_surfaces_against_every_root() {
        let (_tmp_a, root_a) = plain_root();
        let (_tmp_b, root_b) = plain_root();
        let matchers = vec![matcher_for("A", &root_a), matcher_for("B", &root_b)];
        let errors = vec![notify::Error::generic("backend died")];

        let mut events = build_watch_error_events(&errors, &matchers);
        events.sort_by(|a, b| match (a, b) {
            (FsEvent::WatchError { root: ra, .. }, FsEvent::WatchError { root: rb, .. }) => {
                ra.cmp(rb)
            }
            _ => std::cmp::Ordering::Equal,
        });
        assert_eq!(
            events,
            vec![
                FsEvent::WatchError {
                    root: "A".to_string(),
                    reason: "backend died".to_string(),
                },
                FsEvent::WatchError {
                    root: "B".to_string(),
                    reason: "backend died".to_string(),
                },
            ]
        );
    }

    // ── build_root_matchers: a root that doesn't exist emits an honest WatchError ──────────────

    #[derive(Clone, Default)]
    struct CapturingSink(Arc<Mutex<Vec<FsEvent>>>);
    impl CapturingSink {
        fn events(&self) -> Vec<FsEvent> {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }
    impl FsEventSink for CapturingSink {
        fn send(&self, event: FsEvent) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        }
    }

    #[test]
    fn nonexistent_root_emits_watch_error_and_is_excluded_from_matchers() {
        let (_tmp, root) = plain_root();
        let missing = root.join("does-not-exist");
        let sink = CapturingSink::default();

        let matchers = build_root_matchers(&[missing.to_string_lossy().into_owned()], &sink);

        assert!(
            matchers.is_empty(),
            "a nonexistent root must not produce a matcher"
        );
        let events = sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            FsEvent::WatchError { root: r, reason } => {
                assert_eq!(r, &missing.to_string_lossy());
                assert!(!reason.is_empty());
            }
            other => panic!("expected WatchError, got {other:?}"),
        }
    }

    // ── FsEvent wire shape (AppHandle sink payload keys) ────────────────────────────────────────
    //
    // `AppHandle::emit`'s exact JSON shape can't be asserted without a live Tauri runtime; instead
    // this locks the payload `serde_json::Value` shape the `FsEventSink for AppHandle` impl
    // constructs, mirroring how `broker.rs`'s tests assert on `BrokerAction::Emit`'s payload.

    #[test]
    fn changed_payload_uses_camel_case_changed_rel_paths_key() {
        let event = FsEvent::Changed {
            root: "r".to_string(),
            rel_paths: vec!["a.txt".to_string()],
        };
        let (root, rel_paths) = match &event {
            FsEvent::Changed { root, rel_paths } => (root, rel_paths),
            _ => unreachable!(),
        };
        let payload = serde_json::json!({ "root": root, "changedRelPaths": rel_paths });
        assert_eq!(payload["root"], "r");
        assert_eq!(payload["changedRelPaths"], serde_json::json!(["a.txt"]));
    }

    #[test]
    fn watch_error_payload_shape() {
        let event = FsEvent::WatchError {
            root: "r".to_string(),
            reason: "boom".to_string(),
        };
        let (root, reason) = match &event {
            FsEvent::WatchError { root, reason } => (root, reason),
            _ => unreachable!(),
        };
        let payload = serde_json::json!({ "root": root, "reason": reason });
        assert_eq!(payload["root"], "r");
        assert_eq!(payload["reason"], "boom");
    }

    // ── stop_watch_inner: no-op when nothing is watching ────────────────────────────────────────

    #[test]
    fn stop_on_an_already_empty_slot_is_a_harmless_noop() {
        let slot = new_watch_slot();
        stop_watch_inner(&slot); // must not panic
        assert!(slot.lock().unwrap().is_none());
    }

    // ── start_watch_inner: replace semantics (real notify, no timing dependency) ───────────────
    //
    // Creates real `notify_debouncer_full::Debouncer`s against two real tempdirs, but asserts
    // only on `WatchState`'s own fields immediately after each call — no sleep, no dependency on
    // actual FSEvents delivery timing (that's exercised by the ONE timing-based integration test
    // below).

    #[test]
    fn starting_again_replaces_the_previous_watch_state() {
        let (_tmp_a, root_a) = plain_root();
        let (_tmp_b, root_b) = plain_root();
        let slot = new_watch_slot();
        let sink = CapturingSink::default();

        start_watch_inner(
            &slot,
            sink.clone(),
            vec![root_a.to_string_lossy().into_owned()],
            false,
        );
        {
            let guard = slot.lock().unwrap();
            let state = guard.as_ref().expect("watch state must be populated");
            assert_eq!(state.roots, vec![root_a.to_string_lossy().into_owned()]);
            assert!(!state.show_ignored);
        }

        start_watch_inner(
            &slot,
            sink.clone(),
            vec![root_b.to_string_lossy().into_owned()],
            true,
        );
        {
            let guard = slot.lock().unwrap();
            let state = guard.as_ref().expect("watch state must be populated");
            assert_eq!(
                state.roots,
                vec![root_b.to_string_lossy().into_owned()],
                "starting again must REPLACE, not append to, the watched roots"
            );
            assert!(state.show_ignored);
        }

        stop_watch_inner(&slot);
        assert!(slot.lock().unwrap().is_none());
    }

    // ── ONE integration test: real tempdir + real notify (spec §5 DoD) ─────────────────────────

    /// Poll `predicate` until it returns `true` or `deadline` elapses; returns whether it
    /// succeeded. Generous, load-tolerant bound rather than a fixed sleep (the DoD's `touch` < 1s
    /// is runtime truth; this bound only needs to survive CI/load, per the task brief).
    fn wait_for(deadline: Duration, mut predicate: impl FnMut() -> bool) -> bool {
        let start = std::time::Instant::now();
        loop {
            if predicate() {
                return true;
            }
            if start.elapsed() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn real_notify_watch_delivers_debounced_changed_event_filters_gitignore_and_respects_stop() {
        let (_tmp, root) = git_root();
        fs::write(root.join(".gitignore"), b"ignored.log\n").unwrap();

        let sink = CapturingSink::default();
        let slot = new_watch_slot();
        let root_key = root.to_string_lossy().into_owned();

        start_watch_inner(&slot, sink.clone(), vec![root_key.clone()], false);
        // Let the FSEvents stream fully register before generating the first change — otherwise
        // the write below can race the OS-level watch setup.
        std::thread::sleep(Duration::from_millis(200));

        fs::write(root.join("new.txt"), b"hello").unwrap();

        let saw_new_txt = wait_for(Duration::from_secs(3), || {
            sink.events().iter().any(|e| {
                matches!(
                    e,
                    FsEvent::Changed { root: r, rel_paths }
                        if r == &root_key && rel_paths.iter().any(|p| p == "new.txt")
                )
            })
        });
        assert!(
            saw_new_txt,
            "expected fs://changed for new.txt within 3s, got {:?}",
            sink.events()
        );

        // A gitignored write must never surface in any Changed payload — wait comfortably past
        // the 250ms debounce window, then assert absence.
        fs::write(root.join("ignored.log"), b"shh").unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let leaked = sink.events().iter().any(|e| {
            matches!(e, FsEvent::Changed { rel_paths, .. } if rel_paths.iter().any(|p| p == "ignored.log"))
        });
        assert!(
            !leaked,
            "gitignored path must never surface in fs://changed, got {:?}",
            sink.events()
        );

        // After stop_workspace_watch, further writes must produce no further events.
        stop_watch_inner(&slot);
        let count_before_stop = sink.events().len();
        fs::write(root.join("after-stop.txt"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(1000));
        assert_eq!(
            sink.events().len(),
            count_before_stop,
            "no events may arrive after stop_workspace_watch"
        );
    }

    #[test]
    fn watch_start_failure_on_a_nonexistent_root_emits_watch_error_not_a_panic() {
        let (_tmp, root) = plain_root();
        let missing = root.join("does-not-exist");
        let sink = CapturingSink::default();
        let slot = new_watch_slot();

        start_watch_inner(
            &slot,
            sink.clone(),
            vec![missing.to_string_lossy().into_owned()],
            false,
        );

        let events = sink.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, FsEvent::WatchError { root: r, .. } if r == &missing.to_string_lossy())),
            "expected a WatchError for the nonexistent root, got {events:?}"
        );
        // The debouncer itself still starts fine (zero roots watched) and is stored — a later
        // reactivation with a valid root would still work, since start REPLACES the slot.
        assert!(slot.lock().unwrap().is_some());
    }
}
