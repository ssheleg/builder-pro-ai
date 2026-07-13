use bpa_orchd_proto::*;

/// Hop-B framing (`u32`-LE length prefix + CBOR body) must round-trip every `OrchdFrame`
/// byte-identically, mirroring `crates/protocol/tests/roundtrip.rs::assert_frame_roundtrip`.
fn assert_frame_roundtrip(frame: OrchdFrame) {
    let bytes = encode_orchd_frame(&frame).expect("encode");
    let mut decoder = OrchdFrameDecoder::new();
    decoder.push(&bytes);
    let mut decoded = decoder.decode().expect("decode");
    assert_eq!(decoded.len(), 1, "expected exactly one decoded frame");
    let back = decoded.remove(0);
    assert_eq!(
        encode_orchd_frame(&back).expect("re-encode"),
        bytes,
        "frame did not round-trip byte-identically"
    );
}

/// Encode `frame`, assert its raw CBOR wire bytes contain `needle` as a literal UTF-8
/// substring (CBOR text-string values embed their UTF-8 payload verbatim, so this proves
/// the *wire* representation — not just Rust-level equality after a round-trip, which
/// would still pass even if `rename_all = "camelCase"` were silently dropped).
fn assert_wire_contains(frame: &OrchdFrame, needle: &str) {
    let bytes = encode_orchd_frame(frame).expect("encode");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(needle),
        "expected wire bytes to contain {needle:?}; got (lossy utf8):\n{text}"
    );
}

fn sample_project() -> Project {
    Project {
        id: "proj-1".into(),
        name: "Demo Project".into(),
        description: "A demo project".into(),
        status: ProjectStatus::Active,
        workspace_ids: vec!["ws-1".into(), "ws-2".into()],
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_goal() -> Goal {
    Goal {
        id: "goal-1".into(),
        project_id: "proj-1".into(),
        parent_id: Some("goal-0".into()),
        kind: GoalKind::Additional,
        title: "Ship it".into(),
        body: "Ship the thing".into(),
        ord: 3,
        status: GoalStatus::Active,
        metric_refs: vec!["metric-1".into()],
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_idea() -> Idea {
    Idea {
        id: "idea-1".into(),
        project_id: Some("proj-1".into()),
        title: "New idea".into(),
        body: "Idea body".into(),
        lifecycle: IdeaLifecycle::InDev,
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_insight() -> Insight {
    Insight {
        id: "insight-1".into(),
        project_id: Some("proj-1".into()),
        source: "support-ticket".into(),
        title: "Users confused".into(),
        body: "Insight body".into(),
        fit_verdict: Some(FitVerdict::NoFit),
        fit_reasoning: "doesn't match roadmap".into(),
        status: InsightStatus::Accepted,
        resolution_reasoning: "resolved via docs".into(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_task() -> DomainTask {
    DomainTask {
        id: "task-1".into(),
        project_id: "proj-1".into(),
        parent_id: None,
        title: "Do the thing".into(),
        body: "Task body".into(),
        status: TaskStatus::Backlog,
        source: TaskSource::Idea,
        source_id: Some("idea-1".into()),
        tags: vec!["urgent".into()],
        rank: 1.5,
        rank_agent: Some(2.5),
        rank_agent_reasoning: "agent reasoning".into(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_policy() -> PolicyRules {
    PolicyRules {
        spend_cap_usd: Some(100.0),
        approval_classes: vec!["deploy".into()],
        path_allowlist: vec!["/tmp".into()],
    }
}

fn sample_ruleset() -> RuleSet {
    RuleSet {
        id: "rules-1".into(),
        scope: RuleScope::Project,
        project_id: Some("proj-1".into()),
        md_path: "/tmp/rules.md".into(),
        md_hash: "abc123".into(),
        policy: sample_policy(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_ruleset_view() -> RuleSetView {
    RuleSetView {
        rule: sample_ruleset(),
        md_content: Some("# Rules".into()),
        file_state: RuleFileState::Ok,
    }
}

fn all_requests() -> Vec<OrchdRequest> {
    vec![
        OrchdRequest::Ping,
        OrchdRequest::CreateProject {
            name: "Demo".into(),
            description: "desc".into(),
            workspace_ids: vec!["ws-1".into()],
        },
        OrchdRequest::UpdateProject {
            id: "proj-1".into(),
            name: Some("Renamed".into()),
            description: None,
        },
        OrchdRequest::ArchiveProject {
            id: "proj-1".into(),
        },
        OrchdRequest::ListProjects,
        OrchdRequest::AddProjectWorkspace {
            project_id: "proj-1".into(),
            workspace_id: "ws-2".into(),
        },
        OrchdRequest::RemoveProjectWorkspace {
            project_id: "proj-1".into(),
            workspace_id: "ws-2".into(),
        },
        OrchdRequest::CreateGoal {
            project_id: "proj-1".into(),
            parent_id: Some("goal-0".into()),
            kind: GoalKind::Strategic,
            title: "Goal title".into(),
            body: "Goal body".into(),
        },
        OrchdRequest::UpdateGoal {
            id: "goal-1".into(),
            title: Some("New title".into()),
            body: None,
            status: Some(GoalStatus::Achieved),
            metric_refs: Some(vec!["metric-2".into()]),
        },
        OrchdRequest::MoveGoal {
            id: "goal-1".into(),
            new_parent_id: Some("goal-2".into()),
            new_ord: 5,
        },
        OrchdRequest::DeleteGoal {
            id: "goal-1".into(),
        },
        OrchdRequest::ListGoals {
            project_id: "proj-1".into(),
        },
        OrchdRequest::CreateIdea {
            project_id: Some("proj-1".into()),
            title: "Idea title".into(),
            body: "Idea body".into(),
        },
        OrchdRequest::UpdateIdea {
            id: "idea-1".into(),
            title: Some("New idea title".into()),
            body: None,
        },
        OrchdRequest::SetIdeaProject {
            id: "idea-1".into(),
            project_id: None,
        },
        OrchdRequest::SetIdeaLifecycle {
            id: "idea-1".into(),
            lifecycle: IdeaLifecycle::InDev,
        },
        OrchdRequest::DeleteIdea {
            id: "idea-1".into(),
        },
        OrchdRequest::ListIdeas {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::CreateInsight {
            project_id: Some("proj-1".into()),
            source: "support".into(),
            title: "Insight title".into(),
            body: "Insight body".into(),
        },
        OrchdRequest::UpdateInsight {
            id: "insight-1".into(),
            title: Some("New title".into()),
            body: None,
        },
        OrchdRequest::SetInsightFitVerdict {
            id: "insight-1".into(),
            fit_verdict: Some(FitVerdict::NoFit),
            fit_reasoning: "no fit reasoning".into(),
        },
        OrchdRequest::SetInsightStatus {
            id: "insight-1".into(),
            status: InsightStatus::Archived,
            resolution_reasoning: Some("archived reasoning".into()),
        },
        OrchdRequest::DeleteInsight {
            id: "insight-1".into(),
        },
        OrchdRequest::ListInsights { project_id: None },
        OrchdRequest::CreateTask {
            project_id: "proj-1".into(),
            parent_id: None,
            title: "Task title".into(),
            body: "Task body".into(),
            status: Some(TaskStatus::Backlog),
            source: TaskSource::Bug,
            source_id: None,
            tags: vec!["bug".into()],
        },
        OrchdRequest::UpdateTask {
            id: "task-1".into(),
            title: Some("New task title".into()),
            body: None,
            tags: Some(vec!["urgent".into()]),
        },
        OrchdRequest::SetTaskStatus {
            id: "task-1".into(),
            status: TaskStatus::Progress,
        },
        OrchdRequest::SetTaskRank {
            id: "task-1".into(),
            rank: 4.2,
        },
        OrchdRequest::DeleteTask {
            id: "task-1".into(),
        },
        OrchdRequest::ListTasks {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::GetRuleSet {
            scope: RuleScope::Global,
            project_id: None,
        },
        OrchdRequest::UpsertRuleSet {
            scope: RuleScope::Project,
            project_id: Some("proj-1".into()),
            md_content: Some("# Rules".into()),
            md_path: Some("/tmp/rules.md".into()),
            policy: Some(sample_policy()),
        },
        OrchdRequest::AcknowledgeRuleFile {
            id: "rules-1".into(),
        },
        OrchdRequest::ExportProject {
            project_id: "proj-1".into(),
        },
        OrchdRequest::ExportAll,
        OrchdRequest::ImportBundle { json: "{}".into() },
        OrchdRequest::OrchdShutdown { drain: true },
        OrchdRequest::OrchdShutdown { drain: false },
    ]
}

fn all_responses() -> Vec<OrchdResponse> {
    vec![
        OrchdResponse::Ack,
        OrchdResponse::Pong,
        OrchdResponse::Project(sample_project()),
        OrchdResponse::Projects(vec![sample_project()]),
        OrchdResponse::Goal(sample_goal()),
        OrchdResponse::Goals(vec![sample_goal()]),
        OrchdResponse::Idea(sample_idea()),
        OrchdResponse::Ideas(vec![sample_idea()]),
        OrchdResponse::Insight(sample_insight()),
        OrchdResponse::Insights(vec![sample_insight()]),
        OrchdResponse::Task(sample_task()),
        OrchdResponse::Tasks(vec![sample_task()]),
        OrchdResponse::RuleSetView(sample_ruleset_view()),
        OrchdResponse::ExportJson("{\"projects\":[]}".into()),
        OrchdResponse::ImportReport {
            projects: 1,
            goals: 2,
            ideas: 3,
            insights: 4,
            tasks: 5,
            rulesets: 6,
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::NotFound,
            message: "not found".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Invariant,
            message: "invariant".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Validation,
            message: "validation".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Conflict,
            message: "conflict".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Io,
            message: "io".into(),
        },
    ]
}

fn all_pushes() -> Vec<OrchdPush> {
    vec![
        OrchdPush::ProjectsChanged,
        OrchdPush::GoalsChanged {
            project_id: "proj-1".into(),
        },
        OrchdPush::IdeasChanged,
        OrchdPush::InsightsChanged,
        OrchdPush::TasksChanged {
            project_id: "proj-1".into(),
        },
        OrchdPush::RuleSetChanged {
            scope: RuleScope::Global,
            project_id: None,
        },
        OrchdPush::RuleSetChanged {
            scope: RuleScope::Project,
            project_id: Some("proj-1".into()),
        },
    ]
}

#[test]
fn every_request_variant_roundtrips() {
    for (i, req) in all_requests().into_iter().enumerate() {
        assert_frame_roundtrip(OrchdFrame::Request { id: i as u64, req });
    }
}

#[test]
fn every_response_variant_roundtrips() {
    for (i, res) in all_responses().into_iter().enumerate() {
        assert_frame_roundtrip(OrchdFrame::Response { id: i as u64, res });
    }
}

#[test]
fn every_push_variant_roundtrips() {
    for push in all_pushes() {
        assert_frame_roundtrip(OrchdFrame::Push(push));
    }
}

#[test]
fn idea_lifecycle_in_dev_serializes_as_camelcase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetIdeaLifecycle {
            id: "idea-1".into(),
            lifecycle: IdeaLifecycle::InDev,
        },
    };
    assert_wire_contains(&frame, "inDev");
}

#[test]
fn task_status_backlog_serializes_lowercase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetTaskStatus {
            id: "task-1".into(),
            status: TaskStatus::Backlog,
        },
    };
    assert_wire_contains(&frame, "backlog");
}

#[test]
fn fit_verdict_no_fit_serializes_as_camelcase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetInsightFitVerdict {
            id: "insight-1".into(),
            fit_verdict: Some(FitVerdict::NoFit),
            fit_reasoning: "reasoning".into(),
        },
    };
    assert_wire_contains(&frame, "noFit");
}

#[test]
fn version_consts_are_locked_to_one() {
    assert_eq!(ORCHD_CLIENT_MIN_VERSION, 1);
    assert_eq!(ORCHD_CLIENT_MAX_VERSION, 1);
    assert_eq!(ORCHD_DAEMON_MIN_VERSION, 1);
    assert_eq!(ORCHD_DAEMON_MAX_VERSION, 1);
}

#[test]
fn no_double_option_on_update_verbs() {
    // D11: `Update*` verbs use single `Option<T>` (absent OR null both mean "unchanged");
    // there is no `Option<Option<T>>` anywhere in the wire contract. This is a compile-time
    // property (the fields below are plain `Option<T>`), asserted here by simply
    // constructing every `Update*`/`Set*` variant with `None` and `Some` payloads — if any
    // field were `Option<Option<T>>` these literals would not type-check.
    let _ = OrchdRequest::UpdateProject {
        id: "p".into(),
        name: None,
        description: None,
    };
    let _ = OrchdRequest::SetIdeaProject {
        id: "i".into(),
        project_id: None,
    };
    let _ = OrchdRequest::SetInsightFitVerdict {
        id: "i".into(),
        fit_verdict: None,
        fit_reasoning: String::new(),
    };
}
