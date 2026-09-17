use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, ImportRequest, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    StructuredRunRegistry, captured_stdout_event, detect_binary, native_harness_event,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    CommandInfo, Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, FilesystemImpact, HarnessId,
    IntegrationMode, ModelInfo, ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use futures::stream;
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString};
use uuid::Uuid;
const SOURCE: &str = "pi --mode json";
#[derive(Clone, Default)]
pub struct PiAdapter {
    runs: StructuredRunRegistry,
}
#[async_trait]
impl HarnessAdapter for PiAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Pi
    }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary(
            "pi",
            vec![
                IntegrationMode::StructuredStream,
                IntegrationMode::SessionImport,
                IntegrationMode::Rpc,
            ],
        )
        .await
    }
    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut c = BTreeMap::new();
        for x in [
            Capability::ModelInteractions,
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::ShellCommands,
            Capability::TerminalOutput,
            Capability::FileReads,
            Capability::FileWrites,
            Capability::Patches,
            Capability::Failures,
            Capability::ContextChanges,
            Capability::TokenUsage,
            Capability::Cost,
            Capability::RawEvents,
            Capability::FinalOutput,
        ] {
            c.insert(
                x,
                CapabilityEvidence {
                    level: ProvenanceLevel::Native,
                    source: SOURCE.into(),
                    notes: None,
                },
            );
        }
        Ok(CapabilityReport {
            harness: HarnessId::Pi,
            integration_modes: vec![
                IntegrationMode::StructuredStream,
                IntegrationMode::SessionImport,
                IntegrationMode::Rpc,
            ],
            capabilities: c,
        })
    }
    async fn start(&self, r: RunRequest) -> Result<RunHandle, AdapterError> {
        let (p, a) = command(&r.argv)?;
        let mut s = ProcessSpec::new(p, r.cwd);
        s.args = a;
        self.runs
            .launch(r.run_id, HarnessId::Pi, s, normalize_line)
            .await
    }
    async fn events(&self, r: &RunHandle) -> Result<EventStream, AdapterError> {
        self.runs.events(r.run_id)
    }
    async fn cancel(&self, r: &RunHandle) -> Result<(), AdapterError> {
        self.runs.cancel(r.run_id)
    }
    async fn import(&self, r: ImportRequest) -> Result<EventStream, AdapterError> {
        let text = tokio::fs::read_to_string(&r.path).await?;
        let t = Uuid::new_v4();
        let mut s = 0;
        let mut out = Vec::new();
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            out.extend(normalize_line(r.run_id, t, &mut s, l));
        }
        Ok(Box::pin(stream::iter(out.into_iter().map(Ok))))
    }
}
fn command(argv: &[OsString]) -> Result<(OsString, Vec<OsString>), AdapterError> {
    let Some((p, rest)) = argv.split_first() else {
        return Err(AdapterError::InvalidRequest("missing Pi command".into()));
    };
    let mut a = rest.to_vec();
    if let Some(i) = a.iter().position(|x| x.to_string_lossy() == "--mode") {
        let v = a
            .get(i + 1)
            .map(|x| x.to_string_lossy())
            .ok_or_else(|| AdapterError::InvalidRequest("--mode requires a value".into()))?;
        if v != "json" {
            return Err(AdapterError::InvalidRequest(
                "Pi tracing requires --mode json; RPC is a separate integration".into(),
            ));
        }
    } else {
        a.push("--mode".into());
        a.push("json".into());
    }
    Ok((p.clone(), a))
}
pub fn normalize_line(run: Uuid, trace: Uuid, seq: &mut u64, line: &str) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![captured_stdout_event(run, trace, seq, HarnessId::Pi, line)];
    };
    match raw.get("type").and_then(Value::as_str).unwrap_or("unknown") {
        "session" => vec![native(
            run,
            trace,
            seq,
            EventKind::RunStarted,
            json!({"session_id":raw.get("id"),"cwd":raw.get("cwd"),"version":raw.get("version")}),
            raw,
        )],
        "agent_start" | "turn_start" => vec![native(
            run,
            trace,
            seq,
            EventKind::Checkpoint,
            raw.clone(),
            raw,
        )],
        "agent_end" => vec![native(
            run,
            trace,
            seq,
            EventKind::RunCompleted,
            json!({"messages":raw.get("messages")}),
            raw,
        )],
        "tool_execution_start" => normalize_tool_start(run, trace, seq, raw),
        "tool_execution_update" => vec![native(
            run,
            trace,
            seq,
            EventKind::Checkpoint,
            raw.clone(),
            raw,
        )],
        "tool_execution_end" => {
            let err = raw.get("isError").and_then(Value::as_bool).unwrap_or(false);
            let mut e = native(
                run,
                trace,
                seq,
                EventKind::ToolResult,
                raw.clone(),
                raw.clone(),
            );
            if err {
                e.error = Some(ErrorInfo {
                    message: "Pi tool execution failed".into(),
                    code: None,
                    recoverable: None,
                });
            }
            vec![e]
        }
        "message_update" => normalize_update(run, trace, seq, raw),
        "message_end" => normalize_message(
            run,
            trace,
            seq,
            raw.get("message").cloned().unwrap_or(Value::Null),
            raw,
        ),
        "compaction_start" | "compaction_end" => vec![native(
            run,
            trace,
            seq,
            EventKind::ContextCompacted,
            raw.clone(),
            raw,
        )],
        "queue_update" => vec![native(
            run,
            trace,
            seq,
            EventKind::ContextAdded,
            raw.clone(),
            raw,
        )],
        _ => vec![native(
            run,
            trace,
            seq,
            EventKind::Checkpoint,
            raw.clone(),
            raw,
        )],
    }
}
fn normalize_update(run: Uuid, trace: Uuid, seq: &mut u64, raw: Value) -> Vec<EventEnvelope> {
    let usage = usage(raw.get("usage"));
    let ae = raw
        .get("assistantMessageEvent")
        .cloned()
        .unwrap_or(Value::Null);
    let typ = ae.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = if typ.contains("thinking") {
        EventKind::ReasoningStarted
    } else if typ.contains("text") {
        EventKind::ModelResponse
    } else if typ == "toolcall_start" {
        EventKind::ToolCall
    } else {
        EventKind::Checkpoint
    };
    let mut e = native(run, trace, seq, kind, ae, raw);
    e.usage = usage;
    vec![e]
}
fn normalize_message(
    run: Uuid,
    trace: Uuid,
    seq: &mut u64,
    msg: Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();
    match role {
        "assistant" => {
            if let Some(items) = msg.get("content").and_then(Value::as_array) {
                for b in items {
                    let k = match b.get("type").and_then(Value::as_str) {
                        Some("thinking") => EventKind::ReasoningCompleted,
                        Some("toolCall") => EventKind::ToolCall,
                        _ => EventKind::ModelResponse,
                    };
                    out.push(native(run, trace, seq, k, b.clone(), raw.clone()));
                }
            }
            let mut u = native(
                run,
                trace,
                seq,
                EventKind::ModelUsage,
                json!({"usage":msg.get("usage")}),
                raw.clone(),
            );
            u.usage = usage(msg.get("usage"));
            if let Some(total) = msg.pointer("/usage/cost/total").and_then(Value::as_f64) {
                u.cost = Some(Cost {
                    amount: total,
                    currency: "USD".into(),
                    basis: CostBasis::Reported,
                    price_table_version: None,
                });
            }
            if let Some(model) = msg.get("model").and_then(Value::as_str) {
                u.model = Some(ModelInfo {
                    id: model.into(),
                    provider: msg
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            out.push(u);
        }
        "toolResult" => out.push(native(run, trace, seq, EventKind::ToolResult, msg, raw)),
        "bashExecution" => {
            let cmd = msg.get("command").and_then(Value::as_str).unwrap_or("");
            let mut c = native(
                run,
                trace,
                seq,
                EventKind::ShellCommand,
                json!({"command":cmd}),
                raw.clone(),
            );
            c.command = Some(CommandInfo {
                program: "shell".into(),
                args: if cmd.is_empty() {
                    vec![]
                } else {
                    vec![cmd.into()]
                },
                cwd: None,
                exit_code: msg
                    .get("exitCode")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32),
            });
            out.push(c);
            out.push(native(
                run,
                trace,
                seq,
                EventKind::ShellOutput,
                json!({"output":msg.get("output"),"exit_code":msg.get("exitCode")}),
                raw,
            ));
        }
        "compactionSummary" => out.push(native(
            run,
            trace,
            seq,
            EventKind::ContextCompacted,
            msg,
            raw,
        )),
        "system" => out.push(native(run, trace, seq, EventKind::ContextAdded, msg, raw)),
        _ => {}
    }
    out
}
fn normalize_tool_start(run: Uuid, trace: Uuid, seq: &mut u64, raw: Value) -> Vec<EventEnvelope> {
    let name = raw.get("toolName").and_then(Value::as_str).unwrap_or("");
    let args = raw.get("args").cloned().unwrap_or(Value::Null);
    let mut out = vec![native(
        run,
        trace,
        seq,
        EventKind::ToolCall,
        raw.clone(),
        raw.clone(),
    )];
    match name {
        "bash" => {
            let cmd = args.get("command").and_then(Value::as_str).unwrap_or("");
            let mut e = native(
                run,
                trace,
                seq,
                EventKind::ShellCommand,
                json!({"command":cmd}),
                raw,
            );
            e.command = Some(CommandInfo {
                program: "shell".into(),
                args: if cmd.is_empty() {
                    vec![]
                } else {
                    vec![cmd.into()]
                },
                cwd: None,
                exit_code: None,
            });
            out.push(e);
        }
        "read" => file(&mut out, run, trace, seq, EventKind::FileRead, &args, raw),
        "write" => file(&mut out, run, trace, seq, EventKind::FileWrite, &args, raw),
        "edit" => file(&mut out, run, trace, seq, EventKind::FilePatch, &args, raw),
        _ => {}
    }
    out
}
fn file(
    out: &mut Vec<EventEnvelope>,
    run: Uuid,
    trace: Uuid,
    seq: &mut u64,
    kind: EventKind,
    args: &Value,
    raw: Value,
) {
    let path = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(Value::as_str);
    let mut e = native(run, trace, seq, kind, json!({"path":path}), raw);
    let mut f = FilesystemImpact::default();
    if let Some(p) = path {
        if kind == EventKind::FileRead {
            f.paths_read.push(p.into())
        } else {
            f.paths_written.push(p.into())
        }
    }
    e.filesystem_impact = Some(f);
    out.push(e)
}
fn usage(v: Option<&Value>) -> Option<TokenUsage> {
    let v = v?;
    Some(TokenUsage {
        input_tokens: v
            .get("input")
            .or_else(|| v.get("input_tokens"))
            .and_then(Value::as_u64),
        output_tokens: v
            .get("output")
            .or_else(|| v.get("output_tokens"))
            .and_then(Value::as_u64),
        cached_input_tokens: v.get("cacheRead").and_then(Value::as_u64),
        cache_write_input_tokens: v.get("cacheWrite").and_then(Value::as_u64),
        reasoning_output_tokens: v.get("reasoning").and_then(Value::as_u64),
    })
}
fn native(r: Uuid, t: Uuid, s: &mut u64, k: EventKind, p: Value, raw: Value) -> EventEnvelope {
    native_harness_event(r, t, s, HarnessId::Pi, SOURCE, k, p, raw)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_maps_stream() {
        let f = include_str!("../../../../fixtures/pi/stream.jsonl");
        let mut s = 0;
        let r = Uuid::new_v4();
        let t = Uuid::new_v4();
        let e: Vec<_> = f
            .lines()
            .flat_map(|l| normalize_line(r, t, &mut s, l))
            .collect();
        assert!(e.iter().any(|x| x.kind == EventKind::ShellCommand));
        assert!(e.iter().any(|x| x.kind == EventKind::ModelUsage));
        assert!(e.iter().any(|x| x.cost.is_some()));
        assert!(e.iter().any(|x| x.kind == EventKind::RunCompleted));
    }
}
