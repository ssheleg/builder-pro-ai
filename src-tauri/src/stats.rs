//! Usage & output statistics (SCN-052 / SCN-053, FLW-20, spike A-8).
//!
//! Two independent, honestly-degrading data sources feed the Stats view:
//!
//! - **Usage** — Claude Code's own session logs at `~/.claude/projects/<dir>/<session>.jsonl`
//!   (A-8 contract: assistant entries carry `message.usage.{input_tokens, output_tokens,
//!   cache_creation_input_tokens, cache_read_input_tokens}`, `message.model`, a top-level ISO
//!   `timestamp` and a top-level `cwd` — the REAL workspace path, which is what attribution
//!   uses; the dash-encoded dir name is never parsed). The corpus is large (measured ~5,900
//!   files / 2.5 GB, max 57 MB) so a scan NEVER re-reads the world: a JSON cache remembers
//!   `(mtime, size)` per file and its per-day aggregates, and only changed files are re-parsed
//!   (`ScanOutcome::parsed_files` exists so tests can prove the skip). A corrupt or
//!   version-mismatched cache is discarded and rebuilt — logged, never fatal.
//! - **Git output** — `git -C <workspace root> log --since=… --numstat` per root (SCN-053). A
//!   missing binary or a non-repo root yields `available: false` with the reason — the UI shows
//!   "no git data", never fabricated zeros.
//!
//! Cost is an ESTIMATE (UI labels it so): [`PRICING`] maps model FAMILIES (substring match) to
//! public per-MTok USD rates. Prices drift and new families appear; a family absent from the
//! table contributes tokens but no cost, and the day's `costComplete` flag drops to `false` so
//! the frontend can say "partial estimate" instead of quietly under-reporting. That keeps the
//! dashboard honest without blocking on a pricing source of truth.
//!
//! Command surface mirrors `power.rs`: infallible replies (failures arrive IN the payload's
//! `error` field), heavy work behind `spawn_blocking` so the async runtime never stalls.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};

// ── wire types (hand-mirrored in src/ipc/stats.ts — no ts-rs here by design) ────────────────────

/// One day × one workspace path of aggregated usage. Families are folded together; cost sums
/// only the families [`PRICING`] knows (`cost_complete` is `false` when any family was unknown).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    /// `YYYY-MM-DD` (UTC — first 10 chars of the entry's ISO timestamp).
    pub day: String,
    /// The session's real working directory (A-8: top-level `cwd`), the attribution key.
    pub cwd: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// Estimated USD for the priced share of this bucket (None when NO family was priced).
    pub est_cost_usd: Option<f64>,
    /// `false` when at least one contributing model family has no pricing row — the frontend
    /// labels the figure "partial" instead of silently under-counting.
    pub cost_complete: bool,
    /// Distinct session files that touched this (day, cwd).
    pub sessions: u32,
}

/// One model FAMILY (opus / sonnet / haiku / fable / other …) of aggregated usage across the
/// whole range — the "per model family" cut (SCN-052; honest naming of the only agent-side
/// dimension the session logs expose, there is no per-instance agent id). Pricing follows the
/// same rules as [`DayUsage`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FamilyUsage {
    /// `model_family` key: "opus" | "sonnet" | "haiku" | "fable" | "other" | …
    pub family: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// Estimated USD for this family (None when the family has no pricing row).
    pub est_cost_usd: Option<f64>,
    /// Distinct session files that touched this family in range.
    pub sessions: u32,
}

/// `stats_usage` reply. `error` is the whole-scan failure surface (projects dir unreadable …);
/// per-file parse problems are tolerated line-by-line and never fail the scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    /// Unix ms at scan completion — the view's "as of" stamp (SCN-053 freshness rule).
    pub as_of: i64,
    pub days: Vec<DayUsage>,
    /// Per-model-family cut for the range (SCN-052 "per model family" tiles/table).
    pub families: Vec<FamilyUsage>,
    pub error: Option<String>,
}

/// `stats_git` reply, one per requested root (SCN-053). `available: false` + `reason` is the
/// honest "no git data" path — counts are only meaningful when `available` is `true`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitStats {
    pub root: String,
    pub commits: u32,
    pub added: u64,
    pub deleted: u64,
    pub available: bool,
    pub reason: Option<String>,
}

// ── pricing (estimated; see module doc) ─────────────────────────────────────────────────────────

/// USD per **million** tokens: `(family substring, input, output, cache write, cache read)`.
/// Public Anthropic API list prices at the time of writing — they DRIFT; update rows rather than
/// trusting them blindly. A model whose id matches no row is counted token-wise but priced as
/// unknown (see `DayUsage::cost_complete`) — honesty over invented numbers. Substring match on
/// the lowercased model id ("claude-opus-4-8" → "opus").
const PRICING: &[(&str, f64, f64, f64, f64)] = &[
    ("opus", 15.0, 75.0, 18.75, 1.50),
    ("sonnet", 3.0, 15.0, 3.75, 0.30),
    ("haiku", 1.0, 5.0, 1.25, 0.10),
];

/// Model id → family key used for bucketing and pricing. Unknown ids keep their own family name
/// ("fable" today) so a future pricing row starts working without a cache rebuild.
fn model_family(model: &str) -> String {
    let m = model.to_ascii_lowercase();
    for fam in ["opus", "sonnet", "haiku", "fable"] {
        if m.contains(fam) {
            return fam.to_string();
        }
    }
    "other".to_string()
}

fn price_for(family: &str) -> Option<(f64, f64, f64, f64)> {
    PRICING
        .iter()
        .find(|(f, ..)| *f == family)
        .map(|(_, i, o, w, r)| (*i, *o, *w, *r))
}

// ── incremental scan cache ──────────────────────────────────────────────────────────────────────

/// Bump when the aggregate shape changes — a mismatched cache is discarded wholesale (cheap
/// compared to silently misreading old buckets).
const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct FileAgg {
    mtime_ms: i64,
    size: u64,
    /// (day, cwd, family) → [input, output, cache_write, cache_read].
    days: Vec<(String, String, String, [u64; 4])>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ScanCache {
    version: u32,
    files: HashMap<String, FileAgg>,
}

/// What a scan actually did — `parsed_files` lets tests prove the incremental skip (unchanged
/// files must NOT be re-read on the second scan).
#[derive(Debug)]
pub struct ScanOutcome {
    pub days: Vec<DayUsage>,
    pub families: Vec<FamilyUsage>,
    pub parsed_files: usize,
}

fn load_cache(path: &Path) -> ScanCache {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<ScanCache>(&bytes) {
            Ok(c) if c.version == CACHE_VERSION => c,
            Ok(_) | Err(_) => {
                // Corrupt or stale-versioned cache: rebuild from scratch. Logged, never fatal —
                // the only cost is one full re-parse (SCN-052 "loading" covers it).
                tracing::warn!(path = %path.display(), "stats cache unreadable or stale; rebuilding");
                ScanCache {
                    version: CACHE_VERSION,
                    ..Default::default()
                }
            }
        },
        Err(_) => ScanCache {
            version: CACHE_VERSION,
            ..Default::default()
        },
    }
}

fn save_cache(path: &Path, cache: &ScanCache) {
    // Best-effort: a failed cache write only costs the NEXT scan time, so log-and-continue.
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec(cache) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(path, bytes) {
                tracing::warn!(error = %e, "stats cache write failed (scan results still served)");
            }
        }
        Err(e) => tracing::warn!(error = %e, "stats cache serialize failed"),
    }
}

/// Parse one session jsonl into per-(day, cwd, family) token sums. Line-tolerant by contract:
/// non-JSON lines, entries without `message.usage`, and unknown extra keys are all skipped —
/// Claude Code's log format grows fields freely (A-8).
fn parse_session_file(path: &Path) -> Vec<(String, String, String, [u64; 4])> {
    let mut buckets: BTreeMap<(String, String, String), [u64; 4]> = BTreeMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let reader = std::io::BufReader::new(file);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(usage) = v.get("message").and_then(|m| m.get("usage")) else {
            continue;
        };
        let Some(out) = usage.get("output_tokens").and_then(|x| x.as_u64()) else {
            continue;
        };
        let g = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let ts = v.get("timestamp").and_then(|x| x.as_str()).unwrap_or("");
        if ts.len() < 10 {
            continue;
        }
        let day = ts[..10].to_string();
        let cwd = v
            .get("cwd")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let model = v
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let fam = model_family(model);
        let e = buckets.entry((day, cwd, fam)).or_default();
        e[0] += g("input_tokens");
        e[1] += out;
        e[2] += g("cache_creation_input_tokens");
        e[3] += g("cache_read_input_tokens");
    }
    buckets
        .into_iter()
        .map(|((d, c, f), t)| (d, c, f, t))
        .collect()
}

/// Inclusive `YYYY-MM-DD` cutoff for a range key, `None` for `"all"`. Lexicographic compare is
/// sound for zero-padded ISO dates.
fn range_cutoff_day(range: &str, now_ms: i64) -> Option<String> {
    let days = match range {
        "7d" => 7i64,
        "30d" => 30,
        _ => return None,
    };
    let secs = now_ms / 1000 - days * 86_400;
    Some(day_from_unix(secs))
}

/// Unix seconds → UTC `YYYY-MM-DD` without pulling a chrono dependency (civil-from-days
/// algorithm, Howard Hinnant's formulation — exact over the relevant range).
fn day_from_unix(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The full incremental scan: list every `*.jsonl` under `projects_dir`, reuse cached per-file
/// aggregates when `(mtime, size)` is unchanged, re-parse the rest, fold into (day, cwd)
/// buckets, apply the range filter, persist the refreshed cache.
pub fn scan_usage(
    projects_dir: &Path,
    cache_path: &Path,
    range: &str,
    now_ms: i64,
) -> Result<ScanOutcome, String> {
    let mut cache = load_cache(cache_path);
    let mut seen: HashSet<String> = HashSet::new();
    let mut parsed_files = 0usize;

    let dirs = std::fs::read_dir(projects_dir)
        .map_err(|e| format!("cannot read {}: {e}", projects_dir.display()))?;
    for dir in dirs.flatten() {
        let dir_path = dir.path();
        if !dir_path.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir_path) else {
            continue; // one unreadable project dir must not kill the scan
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let key = path.to_string_lossy().to_string();
            let Ok(meta) = f.metadata() else { continue };
            let size = meta.len();
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            seen.insert(key.clone());
            let unchanged = cache
                .files
                .get(&key)
                .map(|c| c.mtime_ms == mtime_ms && c.size == size)
                .unwrap_or(false);
            if !unchanged {
                parsed_files += 1;
                let days = parse_session_file(&path);
                cache.files.insert(
                    key,
                    FileAgg {
                        mtime_ms,
                        size,
                        days,
                    },
                );
            }
        }
    }
    // Sessions deleted on disk leave the cache too — stale rows must not haunt the totals.
    cache.files.retain(|k, _| seen.contains(k));
    save_cache(cache_path, &cache);

    // Fold per-file aggregates into (day, cwd) buckets; families price independently.
    let cutoff = range_cutoff_day(range, now_ms);
    #[derive(Default)]
    struct Bucket {
        t: [u64; 4],
        cost: f64,
        priced_any: bool,
        unpriced_any: bool,
        sessions: u32,
    }
    let mut buckets: BTreeMap<(String, String), Bucket> = BTreeMap::new();
    for agg in cache.files.values() {
        let mut touched: HashSet<(String, String)> = HashSet::new();
        for (day, cwd, fam, t) in &agg.days {
            if let Some(c) = &cutoff {
                if day < c {
                    continue;
                }
            }
            let b = buckets.entry((day.clone(), cwd.clone())).or_default();
            for (slot, add) in b.t.iter_mut().zip(t.iter()) {
                *slot += add;
            }
            match price_for(fam) {
                Some((pi, po, pw, pr)) => {
                    b.priced_any = true;
                    b.cost +=
                        (t[0] as f64 * pi + t[1] as f64 * po + t[2] as f64 * pw + t[3] as f64 * pr)
                            / 1_000_000.0;
                }
                None => b.unpriced_any = true,
            }
            touched.insert((day.clone(), cwd.clone()));
        }
        for k in touched {
            if let Some(b) = buckets.get_mut(&k) {
                b.sessions += 1;
            }
        }
    }
    let days = buckets
        .into_iter()
        .map(|((day, cwd), b)| DayUsage {
            day,
            cwd,
            tokens_in: b.t[0],
            tokens_out: b.t[1],
            cache_write: b.t[2],
            cache_read: b.t[3],
            est_cost_usd: b.priced_any.then_some(b.cost),
            cost_complete: !b.unpriced_any,
            sessions: b.sessions,
        })
        .collect();

    // Per-model-family cut (SCN-052): fold the SAME cached aggregates by family across the range.
    // Family is already retained per-file in `FileAgg.days` (no cache change), it was only folded
    // away above — so this is a second, independent pass over the same data.
    #[derive(Default)]
    struct FamBucket {
        t: [u64; 4],
        cost: f64,
        priced: bool,
        sessions: u32,
    }
    let mut fam_buckets: BTreeMap<String, FamBucket> = BTreeMap::new();
    for agg in cache.files.values() {
        let mut touched: HashSet<String> = HashSet::new();
        for (day, _cwd, fam, t) in &agg.days {
            if let Some(c) = &cutoff {
                if day < c {
                    continue;
                }
            }
            let b = fam_buckets.entry(fam.clone()).or_default();
            for (slot, add) in b.t.iter_mut().zip(t.iter()) {
                *slot += add;
            }
            if let Some((pi, po, pw, pr)) = price_for(fam) {
                b.priced = true;
                b.cost +=
                    (t[0] as f64 * pi + t[1] as f64 * po + t[2] as f64 * pw + t[3] as f64 * pr)
                        / 1_000_000.0;
            }
            touched.insert(fam.clone());
        }
        for f in touched {
            if let Some(b) = fam_buckets.get_mut(&f) {
                b.sessions += 1;
            }
        }
    }
    // Largest spend first, ties broken by token volume then name — a stable, meaningful order.
    let mut families: Vec<FamilyUsage> = fam_buckets
        .into_iter()
        .map(|(family, b)| FamilyUsage {
            family,
            tokens_in: b.t[0],
            tokens_out: b.t[1],
            cache_write: b.t[2],
            cache_read: b.t[3],
            est_cost_usd: b.priced.then_some(b.cost),
            sessions: b.sessions,
        })
        .collect();
    families.sort_by(|a, b| {
        let ca = a.est_cost_usd.unwrap_or(0.0);
        let cb = b.est_cost_usd.unwrap_or(0.0);
        cb.partial_cmp(&ca)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((b.tokens_in + b.tokens_out).cmp(&(a.tokens_in + a.tokens_out)))
            .then(a.family.cmp(&b.family))
    });

    Ok(ScanOutcome {
        days,
        families,
        parsed_files,
    })
}

// ── git output stats (SCN-053) ──────────────────────────────────────────────────────────────────

/// `git log --numstat` over one workspace root. Every failure mode (no binary, not a repo, git
/// error) lands in `available:false` + `reason` — the caller renders "no git data", never zeros.
pub fn git_stats_for_root(root: &str, range: &str, now_ms: i64) -> GitStats {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .arg("log")
        .arg("--numstat")
        .arg("--pretty=%H");
    if let Some(cutoff) = range_cutoff_day(range, now_ms) {
        cmd.arg(format!("--since={cutoff}T00:00:00Z"));
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return GitStats {
                root: root.to_string(),
                commits: 0,
                added: 0,
                deleted: 0,
                available: false,
                reason: Some(format!("git unavailable: {e}")),
            }
        }
    };
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return GitStats {
            root: root.to_string(),
            commits: 0,
            added: 0,
            deleted: 0,
            available: false,
            reason: Some(if msg.is_empty() {
                "not a git repository".into()
            } else {
                msg
            }),
        };
    }
    let mut commits = 0u32;
    let (mut added, mut deleted) = (0u64, 0u64);
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let line = line.trim();
        if line.len() == 40 && line.bytes().all(|b| b.is_ascii_hexdigit()) {
            commits += 1;
            continue;
        }
        // numstat: "<added>\t<deleted>\t<path>"; binary files show "-" — skipped honestly.
        let mut parts = line.split('\t');
        if let (Some(a), Some(d), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            if let (Ok(a), Ok(d)) = (a.parse::<u64>(), d.parse::<u64>()) {
                added += a;
                deleted += d;
            }
        }
    }
    GitStats {
        root: root.to_string(),
        commits,
        added,
        deleted,
        available: true,
        reason: None,
    }
}

// ── #[tauri::command] surface (registered in lib.rs's generate_handler!) ────────────────────────

fn default_projects_dir() -> PathBuf {
    // `$HOME` is guaranteed for a desktop-session process; the empty-path fallback fails the
    // scan with an honest error rather than probing relative paths.
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(".claude").join("projects")
}

fn cache_path(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("stats-cache.json")
}

/// Scan Claude Code usage for the range. Infallible reply: scan failures arrive in `error`
/// with `days` empty (the view renders the per-source "data unavailable" note, SCN-052).
#[tauri::command]
pub async fn stats_usage(range: String, app: tauri::AppHandle) -> UsageStats {
    let cache = cache_path(&app);
    let res = tauri::async_runtime::spawn_blocking(move || {
        let now = now_unix_ms();
        (
            scan_usage(&default_projects_dir(), &cache, &range, now),
            now,
        )
    })
    .await;
    match res {
        Ok((Ok(outcome), now)) => UsageStats {
            as_of: now,
            days: outcome.days,
            families: outcome.families,
            error: None,
        },
        Ok((Err(e), now)) => UsageStats {
            as_of: now,
            days: Vec::new(),
            families: Vec::new(),
            error: Some(e),
        },
        Err(e) => UsageStats {
            as_of: now_unix_ms(),
            days: Vec::new(),
            families: Vec::new(),
            error: Some(format!("stats worker failed: {e}")),
        },
    }
}

/// Git output stats for the given workspace roots (SCN-053). Per-root honesty — one bad root
/// never fails the others.
#[tauri::command]
pub async fn stats_git(roots: Vec<String>, range: String) -> Vec<GitStats> {
    // Keep the roots for the worker-failure path: a panicked blocking task must surface as an
    // honest per-root "no git data" (available:false + reason), NOT `unwrap_or_default()`'s empty
    // Vec, which the view would read as "success, nothing to show" — a fake-empty (AUD-2026-07-23-12).
    let roots_for_err = roots.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let now = now_unix_ms();
        roots
            .iter()
            .map(|r| git_stats_for_root(r, &range, now))
            .collect::<Vec<_>>()
    })
    .await;
    match res {
        Ok(v) => v,
        Err(e) => roots_for_err
            .into_iter()
            .map(|root| GitStats {
                root,
                commits: 0,
                added: 0,
                deleted: 0,
                available: false,
                reason: Some(format!("git worker failed: {e}")),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_784_000_000_000; // 2026-07-13T?? — fixed so range math is deterministic

    fn write_jsonl(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, lines.join("\n")).unwrap();
        p
    }

    fn entry(
        day: &str,
        cwd: &str,
        model: &str,
        input: u64,
        output: u64,
        cw: u64,
        cr: u64,
    ) -> String {
        format!(
            r#"{{"timestamp":"{day}T10:00:00.000Z","cwd":"{cwd}","message":{{"model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":{cw},"cache_read_input_tokens":{cr}}}}}}}"#
        )
    }

    fn setup(tmp: &Path) -> (PathBuf, PathBuf) {
        let projects = tmp.join("projects");
        let proj_dir = projects.join("-Users-x-proj-a");
        std::fs::create_dir_all(&proj_dir).unwrap();
        (projects, tmp.join("cache.json"))
    }

    #[test]
    fn aggregates_two_files_across_cwds_and_days_skipping_garbage() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        write_jsonl(
            &d,
            "s1.jsonl",
            &[
                entry("2026-07-10", "/x/a", "claude-opus-4-8", 100, 200, 10, 1000),
                "not json at all".to_string(),
                r#"{"type":"queue-operation","timestamp":"2026-07-10T09:00:00Z"}"#.to_string(),
                entry("2026-07-11", "/x/a", "claude-opus-4-8", 1, 2, 3, 4),
            ],
        );
        write_jsonl(
            &d,
            "s2.jsonl",
            &[entry(
                "2026-07-10",
                "/x/b",
                "claude-haiku-4-5",
                50,
                60,
                0,
                0,
            )],
        );

        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        assert_eq!(out.parsed_files, 2);
        assert_eq!(out.days.len(), 3);
        let a10 = out
            .days
            .iter()
            .find(|r| r.day == "2026-07-10" && r.cwd == "/x/a")
            .unwrap();
        assert_eq!(
            (
                a10.tokens_in,
                a10.tokens_out,
                a10.cache_write,
                a10.cache_read
            ),
            (100, 200, 10, 1000)
        );
        assert_eq!(a10.sessions, 1);
        assert!(a10.cost_complete);
        // opus: 100*15 + 200*75 + 10*18.75 + 1000*1.5 per MTok
        let expected = (100.0 * 15.0 + 200.0 * 75.0 + 10.0 * 18.75 + 1000.0 * 1.5) / 1e6;
        assert!((a10.est_cost_usd.unwrap() - expected).abs() < 1e-12);
    }

    #[test]
    fn second_scan_skips_unchanged_files_and_range_filters() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        // NOW_MS is 2026-07-13; the 7d cutoff keeps 2026-07-10 but drops 2026-07-01.
        write_jsonl(
            &d,
            "s1.jsonl",
            &[
                entry("2026-07-01", "/x/a", "claude-opus-4-8", 5, 5, 0, 0),
                entry("2026-07-10", "/x/a", "claude-opus-4-8", 7, 7, 0, 0),
            ],
        );
        let first = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        assert_eq!(first.parsed_files, 1);
        assert_eq!(first.days.len(), 2);

        let second = scan_usage(&projects, &cache, "7d", NOW_MS).unwrap();
        assert_eq!(
            second.parsed_files, 0,
            "unchanged file must be served from cache"
        );
        assert_eq!(second.days.len(), 1);
        assert_eq!(second.days[0].day, "2026-07-10");
    }

    #[test]
    fn corrupt_cache_recovers_by_rebuilding() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        write_jsonl(
            &d,
            "s1.jsonl",
            &[entry("2026-07-10", "/x/a", "claude-opus-4-8", 1, 1, 0, 0)],
        );
        std::fs::write(&cache, b"{ definitely not the cache shape").unwrap();
        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        assert_eq!(out.parsed_files, 1);
        assert_eq!(out.days.len(), 1);
    }

    #[test]
    fn unknown_family_counts_tokens_but_prices_none_and_flags_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        write_jsonl(
            &d,
            "s1.jsonl",
            &[entry("2026-07-10", "/x/a", "claude-fable-5", 10, 10, 0, 0)],
        );
        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        let row = &out.days[0];
        assert_eq!(row.tokens_in, 10);
        assert_eq!(
            row.est_cost_usd, None,
            "fable has no pricing row yet — honest None"
        );
        assert!(!row.cost_complete);
    }

    #[test]
    fn mixed_families_sum_priced_share_and_flag_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        write_jsonl(
            &d,
            "s1.jsonl",
            &[
                entry("2026-07-10", "/x/a", "claude-opus-4-8", 1_000_000, 0, 0, 0),
                entry("2026-07-10", "/x/a", "claude-fable-5", 9, 9, 0, 0),
            ],
        );
        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        let row = &out.days[0];
        assert!(
            (row.est_cost_usd.unwrap() - 15.0).abs() < 1e-9,
            "only the opus share is priced"
        );
        assert!(!row.cost_complete);
    }

    #[test]
    fn family_cut_folds_across_cwds_and_days_priced_independently() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        // opus in two files (distinct sessions) across two cwds + one unpriced fable session.
        write_jsonl(
            &d,
            "s1.jsonl",
            &[
                entry("2026-07-10", "/x/a", "claude-opus-4-8", 1_000_000, 0, 0, 0),
                entry("2026-07-11", "/x/b", "claude-opus-4-8", 1_000_000, 0, 0, 0),
            ],
        );
        write_jsonl(
            &d,
            "s2.jsonl",
            &[entry("2026-07-10", "/x/a", "claude-opus-4-8", 0, 0, 0, 0)],
        );
        write_jsonl(
            &d,
            "s3.jsonl",
            &[entry("2026-07-10", "/x/a", "claude-fable-5", 42, 42, 0, 0)],
        );

        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        // opus sorts first (has cost), fable second (unpriced).
        assert_eq!(out.families.len(), 2);
        let opus = &out.families[0];
        assert_eq!(opus.family, "opus");
        assert_eq!(opus.tokens_in, 2_000_000);
        assert_eq!(opus.sessions, 2, "two distinct opus session files");
        assert!((opus.est_cost_usd.unwrap() - 30.0).abs() < 1e-9); // 2 MTok * $15/MTok input
        let fable = &out.families[1];
        assert_eq!(fable.family, "fable");
        assert_eq!(
            fable.est_cost_usd, None,
            "unpriced family carries tokens, no cost"
        );
        assert_eq!(fable.sessions, 1);
    }

    #[test]
    fn deleted_files_leave_the_totals() {
        let tmp = tempfile::tempdir().unwrap();
        let (projects, cache) = setup(tmp.path());
        let d = projects.join("-Users-x-proj-a");
        let p = write_jsonl(
            &d,
            "s1.jsonl",
            &[entry("2026-07-10", "/x/a", "claude-opus-4-8", 1, 1, 0, 0)],
        );
        scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        std::fs::remove_file(p).unwrap();
        let out = scan_usage(&projects, &cache, "all", NOW_MS).unwrap();
        assert!(
            out.days.is_empty(),
            "stale cache rows for deleted sessions must not survive"
        );
    }

    #[test]
    fn missing_projects_dir_is_an_honest_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = scan_usage(
            &tmp.path().join("nope"),
            &tmp.path().join("c.json"),
            "all",
            NOW_MS,
        )
        .unwrap_err();
        assert!(err.contains("cannot read"));
    }

    #[test]
    fn day_from_unix_matches_known_dates() {
        assert_eq!(day_from_unix(0), "1970-01-01");
        assert_eq!(day_from_unix(1_753_228_800), "2025-07-23");
    }

    // ── git (SCN-053) ───────────────────────────────────────────────────────────────────────────

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_DATE", "2026-07-10T10:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-07-10T10:00:00Z")
            .output()
            .unwrap();
        assert!(
            ok.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn real_repo_counts_commits_and_numstat() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "c1"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "c2"]);

        let s = git_stats_for_root(&root.to_string_lossy(), "all", NOW_MS);
        assert!(s.available, "reason: {:?}", s.reason);
        assert_eq!(s.commits, 2);
        assert_eq!((s.added, s.deleted), (2, 1));
    }

    #[test]
    fn non_repo_root_is_unavailable_with_reason_not_zeros() {
        let tmp = tempfile::tempdir().unwrap();
        let s = git_stats_for_root(&tmp.path().to_string_lossy(), "all", NOW_MS);
        assert!(!s.available);
        assert!(s.reason.is_some());
    }
}
