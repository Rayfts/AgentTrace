use std::{collections::BTreeMap, ffi::OsString};

use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, ImportRequest, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    captured_stdout_event, detect_binary, native_harness_event, StructuredRunRegistry,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    CommandInfo, ErrorInfo, EventEnvelope, EventKind, FilesystemImpact, HarnessId,
    IntegrationMode, ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use futures::stream;
use serde_json::{json, Value};
use uuid::Uuid;

const SOURCE: &str = "codex exec --json";

#[derive(Clone, Default)]
pub struct CodexAdapter {
    runs: StructuredRunRegistry,
}

#[async_trait]
impl HarnessAdapter for CodexAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary(
            "codex",
            vec![IntegrationMode::StructuredStream, IntegrationMode::SessionImport],
        )
        .await
    }

    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut capabilities = BTreeMap::new();
        for capability in [
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::ShellCommands,
            Capability::TerminalOutput,
            Capability::FileWrites,
            Capability::McpActivity,
            Capability::Failures,
            Capability::Duration,
            Capability::TokenUsage,
            Capability::RawEvents,
            Capability::FinalOutput,
        ] {
            capabilities.insert(
                capability,
                CapabilityEvidence {
                    level: ProvenanceLevel::Native,
                    source: SOURCE.into(),
                    notes: None,
                },
            );
        }
        capabilities.insert(
            Capability::ModelInteractions,
            CapabilityEvidence {
                level: ProvenanceLevel::Native,
                source: "assistant_message items from codex exec --json".into(),
                notes: Some("assistant responses are exposed; full provider request bodies are not claimed".into()),
            },
        );
        capabilities.insert(
            Capability::ContextChanges,
            CapabilityEvidence {
                level: ProvenanceLevel::Unavailable,
                source: SOURCE.into(),
                notes: Some("the exec event stream does not expose complete context contents".into()),
            },
        );
        capabilities.insert(
            Capability::Cost,
            CapabilityEvidence {
                level: ProvenanceLevel::Unavailable,
                source: SOURCE.into(),
                notes: Some("cost is not fabricated from token counts without an explicit price table".into()),
            },
        );

        Ok(CapabilityReport {
            harness: HarnessId::Codex,
            integration_modes: vec![IntegrationMode::StructuredStream, IntegrationMode::SessionImport],
            capabilities,
        })
    }

    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError> {
        let (program, args) = codex_exec_command(&request.argv)?;
        let mut spec = ProcessSpec::new(program, request.cwd);
        spec.args = args;
        self.runs
            .launch(request.run_id, HarnessId::Codex, spec, normalize_line)
            .await
    }

    async fn events(&self, run: &RunHandle) -> Result<EventStream, AdapterError> {
        self.runs.events(run.run_id)
    }

    async fn cancel(&self, run: &RunHandle) -> Result<(), AdapterError> {
        self.runs.cancel(run.run_id)
    }

    async fn import(&self, request: ImportRequest) -> Result<EventStream, AdapterError> {
        let text = tokio::fs::read_to_string(&request.path).await?;
        let trace_id = Uuid::new_v4();
        let mut sequence = 0_u64;
        let mut events = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            events.extend(normalize_line(request.run_id, trace_id, &mut sequence, line));
        }
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn codex_exec_command(argv: &[OsString]) -> Result<(OsString, Vec<OsString>), AdapterError> {
    let Some((program, rest)) = argv.split_first() else {
        return Err(AdapterError::InvalidRequest("missing Codex command".into()));
    };
    let mut args = rest.to_vec();
    let exec_position = args
        .iter()
        .position(|arg| arg.to_string_lossy() == "exec")
        .ok_or_else(|| {
            AdapterError::InvalidRequest(
                "structured Codex tracing requires `codex exec ...`; interactive TUI runs are not silently converted to headless execution".into(),
            )
        })?;
    let has_json = args.iter().any(|arg| {
        matches!(arg.to_string_lossy().as_ref(), "--json" | "--experimental-json")
    });
    if !has_json {
        args.insert(exec_position + 1, OsString::from("--json"));
    }
    Ok((program.clone(), args))
}

pub fn normalize_line(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    line: &str,
) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![captured_stdout_event(
            run_id,
            trace_id,
            sequence,
            HarnessId::Codex,
            line,
        )];
    };

    let event_type = raw.get("type").and_then(Value::as_str).unwrap_or("unknown");
    match event_type {
        "thread.started" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::RunStarted,
            json!({"thread_id": raw.get("thread_id")}),
            raw,
        )],
        "turn.started" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"phase": "turn.started"}),
            raw,
        )],
        "turn.completed" => normalize_turn_completed(run_id, trace_id, sequence, raw),
        "turn.failed" => {
            let message = raw
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Codex turn failed")
                .to_owned();
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::RunFailed,
                json!({"error": raw.get("error")}),
                raw,
            );
            event.error = Some(ErrorInfo { message, code: None, recoverable: None });
            vec![event]
        }
        "item.started" | "item.updated" | "item.completed" => {
            normalize_item(run_id, trace_id, sequence, event_type, raw)
        }
        _ => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"upstream_type": event_type}),
            raw,
        )],
    }
}

fn normalize_turn_completed(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    raw: Value,
) -> Vec<EventEnvelope> {
    let usage = token_usage(raw.get("usage"));
    let mut usage_event = native(
        run_id,
        trace_id,
        sequence,
        EventKind::ModelUsage,
        json!({"usage": raw.get("usage")}),
        raw.clone(),
    );
    usage_event.usage = usage.clone();

    let mut completed = native(
        run_id,
        trace_id,
        sequence,
        EventKind::RunCompleted,
        json!({"usage": raw.get("usage")}),
        raw,
    );
    completed.usage = usage;
    vec![usage_event, completed]
}

fn normalize_item(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    event_type: &str,
    raw: Value,
) -> Vec<EventEnvelope> {
    let item = raw.get("item").cloned().unwrap_or(Value::Null);
    let item_type = item
        .get("item_type")
        .or_else(|| item.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let completed = event_type == "item.completed";

    match item_type {
        "assistant_message" if completed => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::ModelResponse,
            json!({"id": item.get("id"), "text": item.get("text")}),
            raw,
        )],
        "reasoning" => vec![native(
            run_id,
            trace_id,
            sequence,
            if completed { EventKind::ReasoningCompleted } else { EventKind::ReasoningStarted },
            json!({"id": item.get("id"), "text": item.get("text")}),
            raw,
        )],
        "command_execution" => normalize_command(run_id, trace_id, sequence, completed, item, raw),
        "file_change" => normalize_file_change(run_id, trace_id, sequence, item, raw),
        "mcp_tool_call" => vec![native(
            run_id,
            trace_id,
            sequence,
            if completed { EventKind::McpResponse } else { EventKind::McpRequest },
            json!({
                "id": item.get("id"),
                "server": item.get("server"),
                "tool": item.get("tool"),
                "status": item.get("status")
            }),
            raw,
        )],
        "error" => {
            let message = item
                .get("message")
                .or_else(|| item.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("Codex error")
                .to_owned();
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::Error,
                item,
                raw,
            );
            event.error = Some(ErrorInfo { message, code: None, recoverable: None });
            vec![event]
        }
        _ => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"upstream_type": event_type, "item": item}),
            raw,
        )],
    }
}

fn normalize_command(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    completed: bool,
    item: Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    let command = item.get("command").and_then(Value::as_str).unwrap_or("");
    let exit_code = item.get("exit_code").and_then(Value::as_i64).map(|value| value as i32);
    let mut event = native(
        run_id,
        trace_id,
        sequence,
        if completed { EventKind::ShellOutput } else { EventKind::ShellCommand },
        if completed {
            json!({"id": item.get("id"), "output": item.get("aggregated_output"), "exit_code": exit_code})
        } else {
            json!({"id": item.get("id"), "command": command})
        },
        raw,
    );
    event.command = Some(CommandInfo {
        program: "shell".into(),
        args: (!command.is_empty()).then(|| vec![command.to_owned()]).unwrap_or_default(),
        cwd: None,
        exit_code,
    });
    vec![event]
}

fn normalize_file_change(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    item: Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    let Some(changes) = item.get("changes").and_then(Value::as_array) else {
        return vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::FileWrite,
            item,
            raw,
        )];
    };

    changes
        .iter()
        .map(|change| {
            let path = change.get("path").and_then(Value::as_str).unwrap_or("");
            let kind = match change.get("kind").and_then(Value::as_str) {
                Some("add" | "create") => EventKind::FileCreate,
                Some("delete") => EventKind::FileDelete,
                _ => EventKind::FileWrite,
            };
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                kind,
                change.clone(),
                raw.clone(),
            );
            let mut impact = FilesystemImpact::default();
            match kind {
                EventKind::FileCreate => impact.paths_created.push(path.to_owned()),
                EventKind::FileDelete => impact.paths_deleted.push(path.to_owned()),
                _ => impact.paths_written.push(path.to_owned()),
            }
            event.filesystem_impact = Some(impact);
            event
        })
        .collect()
}

fn token_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    Some(TokenUsage {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        cached_input_tokens: value.get("cached_input_tokens").and_then(Value::as_u64),
        cache_write_input_tokens: None,
        reasoning_output_tokens: None,
    })
}

fn native(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    kind: EventKind,
    payload: Value,
    raw: Value,
) -> EventEnvelope {
    native_harness_event(
        run_id,
        trace_id,
        sequence,
        HarnessId::Codex,
        SOURCE,
        kind,
        payload,
        raw,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_json_to_exec_without_changing_interactive_commands() {
        let argv = vec!["codex".into(), "exec".into(), "fix tests".into()];
        let (_, args) = codex_exec_command(&argv).unwrap();
        assert_eq!(args[0], OsString::from("exec"));
        assert_eq!(args[1], OsString::from("--json"));

        let interactive = vec!["codex".into()];
        assert!(codex_exec_command(&interactive).is_err());
    }

    #[test]
    fn fixture_normalizes_commands_files_mcp_usage_and_completion() {
        let fixture = include_str!("../../../../fixtures/codex/exec.jsonl");
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let mut sequence = 0;
        let events: Vec<_> = fixture
            .lines()
            .flat_map(|line| normalize_line(run_id, trace_id, &mut sequence, line))
            .collect();

        assert!(events.iter().any(|event| event.kind == EventKind::ShellCommand));
        assert!(events.iter().any(|event| event.kind == EventKind::ShellOutput));
        assert!(events.iter().any(|event| event.kind == EventKind::FileWrite));
        assert!(events.iter().any(|event| event.kind == EventKind::McpRequest));
        assert!(events.iter().any(|event| event.kind == EventKind::ModelUsage));
        assert!(events.iter().any(|event| event.kind == EventKind::RunCompleted));
        assert!(events.iter().all(|event| event.sequence > 0));
    }
}
