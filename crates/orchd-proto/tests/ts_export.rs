use std::fs;
use std::path::PathBuf;

use bpa_orchd_proto::*;
use ts_rs::TS;

/// Absolute path to the generated shared TS file (repo root `src/ipc/orchd-types.ts`).
fn types_ts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/ipc")
}

fn types_ts_path() -> PathBuf {
    types_ts_dir().join("orchd-types.ts")
}

/// Force every exported entity/enum type to (re)write its TS binding, then read the file
/// back. `export_all_to` disregards `TS_RS_EXPORT_DIR`, so the output directory is
/// deterministic regardless of environment or working directory. Each type's
/// `#[ts(export_to = "orchd-types.ts")]` attribute is just the filename, so every type gets
/// merged into a single `orchd-types.ts` under `out_dir` (mirrors
/// `crates/protocol/tests/ts_export.rs`).
fn export_and_read() -> String {
    Project::export_all_to(types_ts_dir()).expect("export Project");
    ProjectStatus::export_all_to(types_ts_dir()).expect("export ProjectStatus");
    Goal::export_all_to(types_ts_dir()).expect("export Goal");
    GoalKind::export_all_to(types_ts_dir()).expect("export GoalKind");
    GoalStatus::export_all_to(types_ts_dir()).expect("export GoalStatus");
    Idea::export_all_to(types_ts_dir()).expect("export Idea");
    IdeaLifecycle::export_all_to(types_ts_dir()).expect("export IdeaLifecycle");
    Insight::export_all_to(types_ts_dir()).expect("export Insight");
    FitVerdict::export_all_to(types_ts_dir()).expect("export FitVerdict");
    InsightStatus::export_all_to(types_ts_dir()).expect("export InsightStatus");
    DomainTask::export_all_to(types_ts_dir()).expect("export DomainTask");
    TaskStatus::export_all_to(types_ts_dir()).expect("export TaskStatus");
    TaskSource::export_all_to(types_ts_dir()).expect("export TaskSource");
    PolicyRules::export_all_to(types_ts_dir()).expect("export PolicyRules");
    RuleSet::export_all_to(types_ts_dir()).expect("export RuleSet");
    RuleScope::export_all_to(types_ts_dir()).expect("export RuleScope");
    RuleFileState::export_all_to(types_ts_dir()).expect("export RuleFileState");
    RuleSetView::export_all_to(types_ts_dir()).expect("export RuleSetView");
    OrchdErrorCode::export_all_to(types_ts_dir()).expect("export OrchdErrorCode");
    fs::read_to_string(types_ts_path()).expect("read generated orchd-types.ts")
}

/// Whitespace- and quote-insensitive substring check so we assert structure, not
/// formatting (mirrors `crates/protocol/tests/ts_export.rs::contains_normalized`).
fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let strip = |s: &str| s.split_whitespace().collect::<String>().replace('"', "");
    strip(haystack).contains(&strip(needle))
}

#[test]
fn generates_orchd_types_ts_at_shared_path() {
    let ts = export_and_read();
    assert!(!ts.is_empty(), "orchd-types.ts must not be empty");
    assert!(
        types_ts_path().exists(),
        "orchd-types.ts must exist at src/ipc/orchd-types.ts"
    );
}

#[test]
fn project_uses_camelcase_workspace_ids() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "projectId") || ts.contains("export type Project"),
        "sanity: Project type present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "workspaceIds: Array<string>")
            || contains_normalized(&ts, "workspaceIds: string[]"),
        "Project.workspace_ids must serialize as camelCase `workspaceIds`; got:\n{ts}"
    );
    assert!(
        !ts.contains("workspace_ids"),
        "generated TS must not contain snake_case `workspace_ids`"
    );
}

#[test]
fn goal_uses_camelcase_project_id_and_metric_refs() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "projectId: string"),
        "Goal.project_id must serialize as camelCase `projectId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "metricRefs: Array<string>")
            || contains_normalized(&ts, "metricRefs: string[]"),
        "Goal.metric_refs must serialize as camelCase `metricRefs`; got:\n{ts}"
    );
    assert!(
        !ts.contains("metric_refs"),
        "generated TS must not contain snake_case `metric_refs`"
    );
}

#[test]
fn insight_uses_camelcase_fit_verdict() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "fitVerdict"),
        "Insight.fit_verdict must serialize as camelCase `fitVerdict`; got:\n{ts}"
    );
    assert!(
        !ts.contains("fit_verdict"),
        "generated TS must not contain snake_case `fit_verdict`"
    );
}

#[test]
fn ruleset_uses_camelcase_md_path() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "mdPath: string"),
        "RuleSet.md_path must serialize as camelCase `mdPath`; got:\n{ts}"
    );
    assert!(
        !ts.contains("md_path"),
        "generated TS must not contain snake_case `md_path`"
    );
}

#[test]
fn idea_lifecycle_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in [
        "captured",
        "researching",
        "specced",
        "inDev",
        "shipped",
        "archived",
    ] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "IdeaLifecycle must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("in_dev"),
        "generated TS must not contain snake_case `in_dev`"
    );
}

#[test]
fn fit_verdict_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["fit", "noFit", "unknown"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "FitVerdict must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("no_fit"),
        "generated TS must not contain snake_case `no_fit`"
    );
}

#[test]
fn task_status_backlog_tag_present() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "\"backlog\""),
        "TaskStatus must include wire tag \"backlog\"; got:\n{ts}"
    );
}

#[test]
fn domain_task_rank_is_ts_number() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "rank: number"),
        "DomainTask.rank (f64) must be TS `number`; got:\n{ts}"
    );
}

#[test]
fn no_snake_case_leakage_anywhere_in_generated_file() {
    let ts = export_and_read();
    for snake in [
        "project_id",
        "workspace_ids",
        "parent_id",
        "metric_refs",
        "fit_verdict",
        "fit_reasoning",
        "resolution_reasoning",
        "source_id",
        "rank_agent",
        "rank_agent_reasoning",
        "created_at",
        "updated_at",
        "md_path",
        "md_hash",
        "md_content",
        "file_state",
        "spend_cap_usd",
        "approval_classes",
        "path_allowlist",
    ] {
        assert!(
            !ts.contains(snake),
            "generated orchd-types.ts must not contain snake_case `{snake}`; got:\n{ts}"
        );
    }
}

#[test]
fn regenerating_is_byte_identical() {
    let first = export_and_read();
    let second = export_and_read();
    assert_eq!(
        first, second,
        "regenerating orchd-types.ts must be byte-identical (no nondeterminism)"
    );
}
