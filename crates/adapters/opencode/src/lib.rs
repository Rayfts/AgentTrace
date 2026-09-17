use std::{collections::BTreeMap, ffi::OsString};

use agenttrace_adapter_api::{AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream, HarnessAdapter, ImportRequest, RunHandle, RunRequest};
use agenttrace_adapter_common::{captured_stdout_event, detect_binary, native_harness_event, StructuredRunRegistry};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{CommandInfo, Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, FilesystemImpact, HarnessId, IntegrationMode, ProvenanceLevel, TokenUsage};
use async_trait::async_trait;
use futures::stream;
use serde_json::{json, Value};
use uuid::Uuid;

const SOURCE: &str = "opencode run --format json";

#[derive(Clone, Default)]
pub struct OpenCodeAdapter { runs: StructuredRunRegistry }

#[async_trait]
impl HarnessAdapter for OpenCodeAdapter {
    fn id(&self) -> HarnessId { HarnessId::Opencode }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary("opencode", vec![IntegrationMode::StructuredStream, IntegrationMode::SessionImport]).await
    }
    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut capabilities = BTreeMap::new();
        for capability in [Capability::ToolCalls, Capability::ToolResults, Capability::ShellCommands, Capability::TerminalOutput, Capability::FileReads, Capability::FileWrites, Capability::Patches, Capability::Subagents, Capability::Failures, Capability::Duration, Capability::TokenUsage, Capability::Cost, Capability::RawEvents, Capability::FinalOutput] {
            capabilities.insert(capability, CapabilityEvidence { level: ProvenanceLevel::Native, source: SOURCE.into(), notes: None });
        }
        capabilities.insert(Capability::ModelInteractions, CapabilityEvidence { level: ProvenanceLevel::Native, source: "text/reasoning and step-finish records".into(), notes: Some("full provider request bodies are not claimed".into()) });
        capabilities.insert(Capability::McpActivity, CapabilityEvidence { level: ProvenanceLevel::Native, source: "tool records when MCP tools are surfaced by OpenCode".into(), notes: Some("MCP is classified only from explicit MCP tool identity/metadata".into()) });
        capabilities.insert(Capability::Approvals, CapabilityEvidence { level: ProvenanceLevel::Unavailable, source: SOURCE.into(), notes: Some("the non-interactive runner handles permission requests internally and its compact JSON output does not promise a complete approval lifecycle".into()) });
        Ok(CapabilityReport { harness: HarnessId::Opencode, integration_modes: vec![IntegrationMode::StructuredStream, IntegrationMode::SessionImport], capabilities })
    }
    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError> {
        let (program, args) = command(&request.argv)?;
        let mut spec = ProcessSpec::new(program, request.cwd); spec.args = args;
        self.runs.launch(request.run_id, HarnessId::Opencode, spec, normalize_line).await
    }
    async fn events(&self, run: &RunHandle) -> Result<EventStream, AdapterError> { self.runs.events(run.run_id) }
    async fn cancel(&self, run: &RunHandle) -> Result<(), AdapterError> { self.runs.cancel(run.run_id) }
    async fn import(&self, request: ImportRequest) -> Result<EventStream, AdapterError> {
        let text = tokio::fs::read_to_string(&request.path).await?;
        let trace_id = Uuid::new_v4(); let mut sequence = 0; let mut events = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) { events.extend(normalize_line(request.run_id, trace_id, &mut sequence, line)); }
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn command(argv: &[OsString]) -> Result<(OsString, Vec<OsString>), AdapterError> {
    let Some((program, rest)) = argv.split_first() else { return Err(AdapterError::InvalidRequest("missing OpenCode command".into())); };
    let mut args = rest.to_vec();
    if !args.iter().any(|arg| arg.to_string_lossy() == "run") { return Err(AdapterError::InvalidRequest("structured OpenCode tracing requires `opencode run ...`".into())); }
    if args.iter().any(|arg| arg.to_string_lossy() == "--mini") { return Err(AdapterError::InvalidRequest("OpenCode --mini is interactive and cannot be combined with JSON tracing".into())); }
    ensure_pair(&mut args, "--format", "json")?;
    Ok((program.clone(), args))
}

fn ensure_pair(args: &mut Vec<OsString>, flag: &str, expected: &str) -> Result<(), AdapterError> {
    if let Some(index) = args.iter().position(|arg| arg.to_string_lossy() == flag) {
        let value = args.get(index + 1).map(|v| v.to_string_lossy()).ok_or_else(|| AdapterError::InvalidRequest(format!("{flag} requires a value")))?;
        if value != expected { return Err(AdapterError::InvalidRequest(format!("{flag} must be {expected} for tracing"))); }
    } else { args.push(flag.into()); args.push(expected.into()); }
    Ok(())
}

pub fn normalize_line(run_id: Uuid, trace_id: Uuid, sequence: &mut u64, line: &str) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else { return vec![captured_stdout_event(run_id, trace_id, sequence, HarnessId::Opencode, line)]; };
    match raw.get("type").and_then(Value::as_str).unwrap_or("unknown") {
        "text" => vec![native(run_id, trace_id, sequence, EventKind::ModelResponse, raw.get("part").cloned().unwrap_or(Value::Null), raw)],
        "reasoning" => vec![native(run_id, trace_id, sequence, EventKind::ReasoningCompleted, raw.get("part").cloned().unwrap_or(Value::Null), raw)],
        "step_start" => vec![native(run_id, trace_id, sequence, EventKind::Checkpoint, json!({"phase":"step_start","part":raw.get("part")}), raw)],
        "step_finish" => normalize_step_finish(run_id, trace_id, sequence, raw),
        "tool_use" => normalize_tool(run_id, trace_id, sequence, raw),
        "error" => {
            let message = raw.pointer("/error/data/message").or_else(|| raw.pointer("/error/message")).and_then(Value::as_str).unwrap_or("OpenCode error").to_owned();
            let mut event = native(run_id, trace_id, sequence, EventKind::Error, raw.get("error").cloned().unwrap_or(Value::Null), raw);
            event.error = Some(ErrorInfo { message, code: None, recoverable: None }); vec![event]
        }
        kind => vec![native(run_id, trace_id, sequence, EventKind::Checkpoint, json!({"upstream_type":kind}), raw)],
    }
}

fn normalize_step_finish(run_id: Uuid, trace_id: Uuid, sequence: &mut u64, raw: Value) -> Vec<EventEnvelope> {
    let part = raw.get("part").cloned().unwrap_or(Value::Null);
    let tokens = part.get("tokens");
    let usage = tokens.map(|v| TokenUsage { input_tokens: v.get("input").and_then(Value::as_u64), output_tokens: v.get("output").and_then(Value::as_u64), cached_input_tokens: v.pointer("/cache/read").and_then(Value::as_u64), cache_write_input_tokens: v.pointer("/cache/write").and_then(Value::as_u64), reasoning_output_tokens: v.get("reasoning").and_then(Value::as_u64) });
    let mut event = native(run_id, trace_id, sequence, EventKind::ModelUsage, part.clone(), raw);
    event.usage = usage;
    if let Some(cost) = part.get("cost").and_then(Value::as_f64) { event.cost = Some(Cost { amount: cost, currency: "USD".into(), basis: CostBasis::Reported, price_table_version: None }); }
    vec![event]
}

fn normalize_tool(run_id: Uuid, trace_id: Uuid, sequence: &mut u64, raw: Value) -> Vec<EventEnvelope> {
    let part = raw.get("part").cloned().unwrap_or(Value::Null);
    let tool = part.get("tool").and_then(Value::as_str).unwrap_or("unknown");
    let state = part.get("state").cloned().unwrap_or(Value::Null);
    let input = state.get("input").cloned().unwrap_or(Value::Null);
    let mut events = vec![native(run_id, trace_id, sequence, EventKind::ToolCall, json!({"call_id":part.get("callID"),"tool":tool,"input":input}), raw.clone())];
    let status = state.get("status").and_then(Value::as_str).unwrap_or("unknown");
    if matches!(status, "completed" | "error") { events.push(native(run_id, trace_id, sequence, EventKind::ToolResult, json!({"call_id":part.get("callID"),"tool":tool,"status":status,"output":state.get("output"),"error":state.get("error")}), raw.clone())); }
    match tool {
        "bash" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            let mut start = native(run_id, trace_id, sequence, EventKind::ShellCommand, json!({"command":command}), raw.clone());
            start.command = Some(CommandInfo { program:"shell".into(), args: if command.is_empty(){vec![]}else{vec![command.into()]}, cwd:None, exit_code:None }); events.push(start);
            if status == "completed" { events.push(native(run_id, trace_id, sequence, EventKind::ShellOutput, json!({"output":state.get("output")}), raw.clone())); }
        }
        "read" => push_file(&mut events, run_id, trace_id, sequence, EventKind::FileRead, &input, raw.clone()),
        "write" => push_file(&mut events, run_id, trace_id, sequence, EventKind::FileWrite, &input, raw.clone()),
        "edit" | "patch" | "apply_patch" => push_file(&mut events, run_id, trace_id, sequence, EventKind::FilePatch, &input, raw.clone()),
        "task" => events.push(native(run_id, trace_id, sequence, EventKind::SubagentCompleted, part, raw.clone())),
        _ if tool.starts_with("mcp_") || tool.starts_with("mcp__") => events.push(native(run_id, trace_id, sequence, EventKind::McpResponse, part, raw.clone())),
        _ => {}
    }
    events
}

fn push_file(events: &mut Vec<EventEnvelope>, run_id: Uuid, trace_id: Uuid, sequence: &mut u64, kind: EventKind, input: &Value, raw: Value) {
    let path = input.get("filePath").or_else(|| input.get("file_path")).or_else(|| input.get("path")).and_then(Value::as_str);
    let mut event = native(run_id, trace_id, sequence, kind, json!({"path":path}), raw);
    let mut impact = FilesystemImpact::default(); if let Some(path)=path { if kind==EventKind::FileRead { impact.paths_read.push(path.into()); } else { impact.paths_written.push(path.into()); } } event.filesystem_impact=Some(impact); events.push(event);
}

fn native(run_id: Uuid, trace_id: Uuid, sequence: &mut u64, kind: EventKind, payload: Value, raw: Value) -> EventEnvelope { native_harness_event(run_id, trace_id, sequence, HarnessId::Opencode, SOURCE, kind, payload, raw) }

#[cfg(test)] mod tests { use super::*; #[test] fn fixture_maps_tool_and_step_usage(){ let fixture=include_str!("../../../../fixtures/opencode/run.jsonl"); let mut seq=0; let r=Uuid::new_v4(); let t=Uuid::new_v4(); let events:Vec<_>=fixture.lines().flat_map(|l|normalize_line(r,t,&mut seq,l)).collect(); assert!(events.iter().any(|e|e.kind==EventKind::ShellCommand)); assert!(events.iter().any(|e|e.kind==EventKind::ToolResult)); assert!(events.iter().any(|e|e.kind==EventKind::ModelUsage)); } }
