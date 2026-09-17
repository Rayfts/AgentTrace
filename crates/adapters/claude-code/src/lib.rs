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
    CommandInfo, Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, FilesystemImpact,
    HarnessId, IntegrationMode, ModelInfo, ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use futures::stream;
use serde_json::{json, Value};
use uuid::Uuid;

const SOURCE: &str = "claude --output-format stream-json --verbose";

#[derive(Clone, Default)]
pub struct ClaudeCodeAdapter {
    runs: StructuredRunRegistry,
}

#[async_trait]
impl HarnessAdapter for ClaudeCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::ClaudeCode
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary(
            "claude",
            vec![
                IntegrationMode::StructuredStream,
                IntegrationMode::SessionImport,
                IntegrationMode::Hook,
            ],
        )
        .await
    }

    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut capabilities = BTreeMap::new();
        for capability in [
            Capability::ModelInteractions,
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::ShellCommands,
            Capability::TerminalOutput,
            Capability::FileReads,
            Capability::FileWrites,
            Capability::Patches,
            Capability::McpActivity,
            Capability::Subagents,
            Capability::Failures,
            Capability::Duration,
            Capability::Latency,
            Capability::TokenUsage,
            Capability::Cost,
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
            Capability::Approvals,
            CapabilityEvidence {
                level: ProvenanceLevel::Unavailable,
                source: SOURCE.into(),
                notes: Some(
                    "the stream can report denied permissions, but AgentTrace does not claim a complete approval request lifecycle without installing Claude Code hooks"
                        .into(),
                ),
            },
        );
        capabilities.insert(
            Capability::ContextChanges,
            CapabilityEvidence {
                level: ProvenanceLevel::Native,
                source: SOURCE.into(),
                notes: Some(
                    "explicit reset/task lifecycle and exposed system events are native; complete hidden context contents remain unavailable"
                        .into(),
                ),
            },
        );
        capabilities.insert(
            Capability::GitOperations,
            CapabilityEvidence {
                level: ProvenanceLevel::Inferred,
                source: "native Bash tool events".into(),
                notes: Some(
                    "Git operations are classified only when an exposed Bash command invokes git; they are never invented from filesystem changes"
                        .into(),
                ),
            },
        );
        capabilities.insert(
            Capability::Retries,
            CapabilityEvidence {
                level: ProvenanceLevel::Unavailable,
                source: SOURCE.into(),
                notes: Some("retry internals are recorded only when the CLI emits an explicit retry/error event".into()),
            },
        );

        Ok(CapabilityReport {
            harness: HarnessId::ClaudeCode,
            integration_modes: vec![
                IntegrationMode::StructuredStream,
                IntegrationMode::SessionImport,
                IntegrationMode::Hook,
            ],
            capabilities,
        })
    }

    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError> {
        let (program, args) = claude_stream_command(&request.argv)?;
        let mut spec = ProcessSpec::new(program, request.cwd);
        spec.args = args;
        self.runs
            .launch(request.run_id, HarnessId::ClaudeCode, spec, normalize_line)
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

fn claude_stream_command(argv: &[OsString]) -> Result<(OsString, Vec<OsString>), AdapterError> {
    let Some((program, rest)) = argv.split_first() else {
        return Err(AdapterError::InvalidRequest("missing Claude Code command".into()));
    };
    let mut args = rest.to_vec();
    let has_print = args
        .iter()
        .any(|arg| matches!(arg.to_string_lossy().as_ref(), "-p" | "--print"));
    if !has_print {
        return Err(AdapterError::InvalidRequest(
            "structured Claude Code tracing requires `claude -p ...` / `claude --print ...`; interactive sessions should use the hook integration instead of being silently converted to headless mode"
                .into(),
        ));
    }

    let mut output_format_index = None;
    for (index, arg) in args.iter().enumerate() {
        let value = arg.to_string_lossy();
        if value == "--output-format" {
            output_format_index = Some(index);
            break;
        }
        if let Some(format) = value.strip_prefix("--output-format=") {
            if format != "stream-json" {
                return Err(AdapterError::InvalidRequest(format!(
                    "Claude Code output format must be stream-json for tracing, got {format}"
                )));
            }
            output_format_index = Some(index);
            break;
        }
    }

    if let Some(index) = output_format_index {
        if args[index].to_string_lossy() == "--output-format" {
            let format = args
                .get(index + 1)
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or_else(|| AdapterError::InvalidRequest("--output-format requires a value".into()))?;
            if format != "stream-json" {
                return Err(AdapterError::InvalidRequest(format!(
                    "Claude Code output format must be stream-json for tracing, got {format}"
                )));
            }
        }
    } else {
        args.push(OsString::from("--output-format"));
        args.push(OsString::from("stream-json"));
    }

    if !args.iter().any(|arg| arg.to_string_lossy() == "--verbose") {
        args.push(OsString::from("--verbose"));
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
            HarnessId::ClaudeCode,
            line,
        )];
    };

    match raw.get("type").and_then(Value::as_str).unwrap_or("unknown") {
        "assistant" => normalize_assistant(run_id, trace_id, sequence, raw),
        "user" => normalize_user(run_id, trace_id, sequence, raw),
        "system" => normalize_system(run_id, trace_id, sequence, raw),
        "result" => normalize_result(run_id, trace_id, sequence, raw),
        "conversation_reset" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::ContextRemoved,
            json!({"new_conversation_id": raw.get("new_conversation_id")}),
            raw,
        )],
        "rate_limit_event" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"rate_limit_info": raw.get("rate_limit_info")}),
            raw,
        )],
        "hook_event" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"hook_event": raw.get("hook_event")}),
            raw,
        )],
        upstream_type => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"upstream_type": upstream_type}),
            raw,
        )],
    }
}

fn normalize_assistant(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    raw: Value,
) -> Vec<EventEnvelope> {
    let mut events = Vec::new();
    let model_id = raw.pointer("/message/model").and_then(Value::as_str).map(str::to_owned);
    let usage = token_usage(raw.pointer("/message/usage"));

    if let Some(usage_value) = raw.pointer("/message/usage") {
        let mut event = native(
            run_id,
            trace_id,
            sequence,
            EventKind::ModelUsage,
            json!({"usage": usage_value}),
            raw.clone(),
        );
        event.usage = usage.clone();
        attach_model(&mut event, model_id.as_deref());
        events.push(event);
    }

    if let Some(content) = raw.pointer("/message/content").and_then(Value::as_array) {
        for block in content {
            match block.get("type").and_then(Value::as_str).unwrap_or("unknown") {
                "text" => {
                    let mut event = native(
                        run_id,
                        trace_id,
                        sequence,
                        EventKind::ModelResponse,
                        json!({"text": block.get("text")}),
                        raw.clone(),
                    );
                    attach_model(&mut event, model_id.as_deref());
                    events.push(event);
                }
                "thinking" => {
                    let mut event = native(
                        run_id,
                        trace_id,
                        sequence,
                        EventKind::ReasoningCompleted,
                        json!({
                            "thinking": block.get("thinking"),
                            "redacted": block.get("redacted")
                        }),
                        raw.clone(),
                    );
                    attach_model(&mut event, model_id.as_deref());
                    events.push(event);
                }
                "tool_use" => {
                    events.extend(normalize_tool_use(
                        run_id,
                        trace_id,
                        sequence,
                        block,
                        raw.clone(),
                    ));
                }
                "server_tool_use" => {
                    events.push(native(
                        run_id,
                        trace_id,
                        sequence,
                        EventKind::ToolCall,
                        block.clone(),
                        raw.clone(),
                    ));
                }
                block_type if block_type.ends_with("_tool_result") => {
                    events.push(native(
                        run_id,
                        trace_id,
                        sequence,
                        EventKind::ToolResult,
                        block.clone(),
                        raw.clone(),
                    ));
                }
                block_type => events.push(native(
                    run_id,
                    trace_id,
                    sequence,
                    EventKind::Checkpoint,
                    json!({"assistant_block_type": block_type, "block": block}),
                    raw.clone(),
                )),
            }
        }
    }

    if let Some(error) = raw.get("error").and_then(Value::as_str) {
        let mut event = native(
            run_id,
            trace_id,
            sequence,
            EventKind::Error,
            json!({"error": error}),
            raw,
        );
        event.error = Some(ErrorInfo {
            message: error.to_owned(),
            code: Some(error.to_owned()),
            recoverable: None,
        });
        events.push(event);
    }
    events
}

fn normalize_tool_use(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    block: &Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("unknown");
    let input = block.get("input").cloned().unwrap_or(Value::Null);
    let mut events = vec![native(
        run_id,
        trace_id,
        sequence,
        EventKind::ToolCall,
        block.clone(),
        raw.clone(),
    )];

    match name {
        "Bash" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::ShellCommand,
                json!({"tool_use_id": block.get("id"), "command": command}),
                raw,
            );
            event.command = Some(CommandInfo {
                program: "shell".into(),
                args: (!command.is_empty()).then(|| vec![command.to_owned()]).unwrap_or_default(),
                cwd: None,
                exit_code: None,
            });
            events.push(event);
        }
        "Read" => {
            let path = path_from_input(&input);
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::FileRead,
                json!({"tool_use_id": block.get("id"), "path": path}),
                raw,
            );
            let mut impact = FilesystemImpact::default();
            if let Some(path) = path { impact.paths_read.push(path.to_owned()); }
            event.filesystem_impact = Some(impact);
            events.push(event);
        }
        "Write" => {
            let path = path_from_input(&input);
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::FileWrite,
                json!({"tool_use_id": block.get("id"), "path": path}),
                raw,
            );
            let mut impact = FilesystemImpact::default();
            if let Some(path) = path { impact.paths_written.push(path.to_owned()); }
            event.filesystem_impact = Some(impact);
            events.push(event);
        }
        "Edit" | "MultiEdit" => {
            let path = path_from_input(&input);
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::FilePatch,
                json!({"tool_use_id": block.get("id"), "path": path}),
                raw,
            );
            let mut impact = FilesystemImpact::default();
            if let Some(path) = path { impact.paths_written.push(path.to_owned()); }
            event.filesystem_impact = Some(impact);
            events.push(event);
        }
        "Task" | "Agent" => events.push(native(
            run_id,
            trace_id,
            sequence,
            EventKind::SubagentStarted,
            json!({"tool_use_id": block.get("id"), "input": input}),
            raw,
        )),
        _ if name.starts_with("mcp__") => events.push(native(
            run_id,
            trace_id,
            sequence,
            EventKind::McpRequest,
            json!({"tool_use_id": block.get("id"), "name": name, "input": input}),
            raw,
        )),
        _ => {}
    }
    events
}

fn normalize_user(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    raw: Value,
) -> Vec<EventEnvelope> {
    let mut events = Vec::new();
    if let Some(content) = raw.pointer("/message/content").and_then(Value::as_array) {
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let mut event = native(
                    run_id,
                    trace_id,
                    sequence,
                    EventKind::ToolResult,
                    block.clone(),
                    raw.clone(),
                );
                if is_error {
                    event.error = Some(ErrorInfo {
                        message: block.get("content").and_then(Value::as_str).unwrap_or("tool failed").to_owned(),
                        code: None,
                        recoverable: None,
                    });
                }
                events.push(event);
            }
        }
    }

    if let Some(edit) = raw.get("tool_use_result").filter(|value| value.is_object()) {
        if edit.get("filePath").is_some() || edit.get("structuredPatch").is_some() {
            let path = edit.get("filePath").and_then(Value::as_str);
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::FilePatch,
                edit.clone(),
                raw,
            );
            let mut impact = FilesystemImpact::default();
            if let Some(path) = path { impact.paths_written.push(path.to_owned()); }
            event.filesystem_impact = Some(impact);
            events.push(event);
        }
    }
    events
}

fn normalize_system(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    raw: Value,
) -> Vec<EventEnvelope> {
    let subtype = raw.get("subtype").and_then(Value::as_str).unwrap_or("unknown");
    match subtype {
        "init" | "start" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::RunStarted,
            json!({"session_id": raw.get("session_id"), "model": raw.get("model")}),
            raw,
        )],
        "task_started" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::SubagentStarted,
            json!({
                "task_id": raw.get("task_id"),
                "tool_use_id": raw.get("tool_use_id"),
                "description": raw.get("description"),
                "task_type": raw.get("task_type")
            }),
            raw,
        )],
        "task_progress" => {
            let mut event = native(
                run_id,
                trace_id,
                sequence,
                EventKind::Checkpoint,
                json!({
                    "task_id": raw.get("task_id"),
                    "description": raw.get("description"),
                    "usage": raw.get("usage"),
                    "last_tool_name": raw.get("last_tool_name")
                }),
                raw.clone(),
            );
            if let Some(ms) = raw.pointer("/usage/duration_ms").and_then(Value::as_u64) {
                event.duration_ns = Some(ms.saturating_mul(1_000_000));
            }
            vec![event]
        }
        "task_notification" | "task_updated" => {
            let status = raw
                .get("status")
                .or_else(|| raw.pointer("/patch/status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let terminal = matches!(status, "completed" | "failed" | "killed" | "stopped");
            vec![native(
                run_id,
                trace_id,
                sequence,
                if terminal { EventKind::SubagentCompleted } else { EventKind::Checkpoint },
                raw.clone(),
                raw,
            )]
        }
        "compact_boundary" | "compaction" => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::ContextCompacted,
            raw.clone(),
            raw,
        )],
        _ => vec![native(
            run_id,
            trace_id,
            sequence,
            EventKind::Checkpoint,
            json!({"system_subtype": subtype}),
            raw,
        )],
    }
}

fn normalize_result(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    raw: Value,
) -> Vec<EventEnvelope> {
    let mut events = Vec::new();
    let usage = token_usage(raw.get("usage"));
    if raw.get("usage").is_some() {
        let mut usage_event = native(
            run_id,
            trace_id,
            sequence,
            EventKind::ModelUsage,
            json!({"usage": raw.get("usage")}),
            raw.clone(),
        );
        usage_event.usage = usage.clone();
        events.push(usage_event);
    }

    let is_error = raw.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    let mut final_event = native(
        run_id,
        trace_id,
        sequence,
        if is_error { EventKind::RunFailed } else { EventKind::RunCompleted },
        json!({
            "subtype": raw.get("subtype"),
            "result": raw.get("result"),
            "stop_reason": raw.get("stop_reason"),
            "terminal_reason": raw.get("terminal_reason"),
            "num_turns": raw.get("num_turns"),
            "permission_denials": raw.get("permission_denials")
        }),
        raw.clone(),
    );
    final_event.usage = usage;
    if let Some(ms) = raw.get("duration_ms").and_then(Value::as_u64) {
        final_event.duration_ns = Some(ms.saturating_mul(1_000_000));
    }
    if let Some(api_ms) = raw.get("duration_api_ms").and_then(Value::as_u64) {
        final_event.attributes.insert("duration_api_ms".into(), json!(api_ms));
    }
    if let Some(cost) = raw.get("total_cost_usd").and_then(Value::as_f64) {
        final_event.cost = Some(Cost {
            amount: cost,
            currency: "USD".into(),
            basis: CostBasis::Reported,
            price_table_version: None,
        });
    }
    if is_error {
        final_event.error = Some(ErrorInfo {
            message: raw
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or("Claude Code run failed")
                .to_owned(),
            code: raw.get("subtype").and_then(Value::as_str).map(str::to_owned),
            recoverable: None,
        });
    }
    events.push(final_event);
    events
}

fn token_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    Some(TokenUsage {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        cached_input_tokens: value
            .get("cache_read_input_tokens")
            .or_else(|| value.get("cached_input_tokens"))
            .and_then(Value::as_u64),
        cache_write_input_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
        reasoning_output_tokens: value.get("reasoning_output_tokens").and_then(Value::as_u64),
    })
}

fn path_from_input(input: &Value) -> Option<&str> {
    input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(Value::as_str)
}

fn attach_model(event: &mut EventEnvelope, model: Option<&str>) {
    if let Some(model) = model {
        event.model = Some(ModelInfo {
            id: model.to_owned(),
            provider: Some("anthropic".into()),
        });
    }
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
        HarnessId::ClaudeCode,
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
    fn structured_mode_refuses_to_rewrite_interactive_claude() {
        assert!(claude_stream_command(&["claude".into()]).is_err());
        let argv = vec!["claude".into(), "-p".into(), "inspect tests".into()];
        let (_, args) = claude_stream_command(&argv).unwrap();
        assert!(args.iter().any(|arg| arg == "stream-json"));
        assert!(args.iter().any(|arg| arg == "--verbose"));
    }

    #[test]
    fn fixture_normalizes_native_rich_events() {
        let fixture = include_str!("../../../../fixtures/claude-code/stream.jsonl");
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let mut sequence = 0;
        let events: Vec<_> = fixture
            .lines()
            .flat_map(|line| normalize_line(run_id, trace_id, &mut sequence, line))
            .collect();

        for kind in [
            EventKind::RunStarted,
            EventKind::ReasoningCompleted,
            EventKind::ModelResponse,
            EventKind::ToolCall,
            EventKind::ShellCommand,
            EventKind::FilePatch,
            EventKind::ToolResult,
            EventKind::SubagentStarted,
            EventKind::SubagentCompleted,
            EventKind::ModelUsage,
            EventKind::RunCompleted,
        ] {
            assert!(events.iter().any(|event| event.kind == kind), "missing {kind:?}");
        }
        let completed = events
            .iter()
            .find(|event| event.kind == EventKind::RunCompleted)
            .unwrap();
        assert_eq!(completed.cost.as_ref().map(|cost| cost.amount), Some(0.0123));
        assert!(events.iter().all(|event| event.raw_source.is_some() || matches!(event.kind, EventKind::ProcessStdout)));
    }
}
