use agenttrace_adapter_aider::{AiderAdapter, normalize_analytics};
use agenttrace_adapter_api::{Capability, HarnessAdapter, ImportRequest};
use agenttrace_adapter_cline::{ClineAdapter, normalize_line as normalize_cline};
use agenttrace_adapter_continue::{ContinueAdapter, normalize_line as normalize_continue};
use agenttrace_adapter_goose::{GooseAdapter, normalize_line as normalize_goose};
use agenttrace_adapter_roo_code::RooCodeAdapter;
use agenttrace_protocol::{EventEnvelope, EventKind, ProvenanceLevel};
use futures::StreamExt;
use uuid::Uuid;

fn normalize_lines(
    fixture: &str,
    normalizer: fn(Uuid, Uuid, &mut u64, &str) -> Vec<EventEnvelope>,
) -> Vec<EventEnvelope> {
    let run_id = Uuid::new_v4();
    let trace_id = Uuid::new_v4();
    let mut sequence = 0;
    fixture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .flat_map(|line| normalizer(run_id, trace_id, &mut sequence, line))
        .collect()
}

#[tokio::test]
async fn aider_fixture_and_negative_capabilities_match_contract() {
    let events = normalize_lines(
        include_str!("../../../fixtures/aider/analytics.jsonl"),
        normalize_analytics,
    );
    let usage = events
        .iter()
        .find(|event| event.kind == EventKind::ModelUsage)
        .expect("fixture should emit model.usage");
    assert_eq!(
        usage.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(80)
    );
    assert_eq!(usage.cost.as_ref().map(|cost| cost.amount), Some(0.00123));
    assert!(events.iter().any(|event| event.kind == EventKind::Error));

    let report = AiderAdapter::default().capabilities().await.unwrap();
    assert_eq!(report.status(Capability::TokenUsage), ProvenanceLevel::Native);
    assert_eq!(report.status(Capability::Cost), ProvenanceLevel::Native);
    assert_eq!(
        report.status(Capability::ToolCalls),
        ProvenanceLevel::Unavailable
    );
}

#[tokio::test]
async fn goose_fixture_preserves_tools_usage_cost_and_mcp_without_shell_overclaim() {
    let events = normalize_lines(
        include_str!("../../../fixtures/goose/stream.jsonl"),
        normalize_goose,
    );
    assert!(events.iter().any(|event| event.kind == EventKind::ToolCall));
    assert!(events.iter().any(|event| event.kind == EventKind::ToolResult));
    assert!(events.iter().any(|event| event.kind == EventKind::RunCompleted));
    assert!(events.iter().any(|event| event.cost.is_some()));

    let report = GooseAdapter::default().capabilities().await.unwrap();
    assert_eq!(report.status(Capability::McpActivity), ProvenanceLevel::Native);
    assert_eq!(
        report.status(Capability::ShellCommands),
        ProvenanceLevel::Unavailable
    );
}

#[tokio::test]
async fn cline_fixture_normalizes_shell_usage_and_completion() {
    let events = normalize_lines(
        include_str!("../../../fixtures/cline/stream.jsonl"),
        normalize_cline,
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::ShellCommand)
    );
    assert!(events.iter().any(|event| event.kind == EventKind::ShellOutput));
    assert!(events.iter().any(|event| event.kind == EventKind::ModelUsage));
    assert!(events.iter().any(|event| event.kind == EventKind::RunCompleted));

    let report = ClineAdapter::default().capabilities().await.unwrap();
    assert_eq!(report.status(Capability::Subagents), ProvenanceLevel::Native);
    assert_eq!(
        report.status(Capability::McpActivity),
        ProvenanceLevel::Unavailable
    );
}

#[tokio::test]
async fn continue_fixture_stays_final_output_only() {
    let events = normalize_lines(
        include_str!("../../../fixtures/continue/result.jsonl"),
        normalize_continue,
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::ContextCompacted)
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::ModelResponse)
    );
    assert!(events.iter().any(|event| event.kind == EventKind::RunCompleted));

    let report = ContinueAdapter::default().capabilities().await.unwrap();
    assert_eq!(
        report.status(Capability::ToolCalls),
        ProvenanceLevel::Unavailable
    );
    assert_eq!(
        report.status(Capability::TokenUsage),
        ProvenanceLevel::Unavailable
    );
}

#[tokio::test]
async fn roo_fixture_imports_history_without_inventing_timing() {
    let fixture = include_str!("../../../fixtures/roo-code/task.json");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), fixture).unwrap();

    let adapter = RooCodeAdapter;
    let mut stream = adapter
        .import(ImportRequest {
            run_id: Uuid::new_v4(),
            path: file.path().to_path_buf(),
        })
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }

    assert!(events.iter().any(|event| event.kind == EventKind::ToolCall));
    assert!(events.iter().any(|event| event.kind == EventKind::ToolResult));
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::ReasoningCompleted)
    );
    assert!(events.iter().all(|event| event.raw_source.is_some()));

    let report = adapter.capabilities().await.unwrap();
    assert_eq!(
        report.status(Capability::Duration),
        ProvenanceLevel::Unavailable
    );
    assert_eq!(
        report.status(Capability::ShellCommands),
        ProvenanceLevel::Unavailable
    );
}
