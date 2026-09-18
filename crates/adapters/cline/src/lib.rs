use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    StructuredRunRegistry, captured_stdout_event, detect_binary, native_harness_event,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    CommandInfo, Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, FilesystemImpact, HarnessId,
    IntegrationMode, ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;
const SOURCE: &str = "cline --json NDJSON";
#[derive(Clone, Default)]
pub struct ClineAdapter {
    runs: StructuredRunRegistry,
}
#[async_trait]
impl HarnessAdapter for ClineAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Cline
    }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary(
            "cline",
            vec![IntegrationMode::StructuredStream, IntegrationMode::Sdk],
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
            Capability::Subagents,
            Capability::Retries,
            Capability::Failures,
            Capability::Duration,
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
            harness: HarnessId::Cline,
            integration_modes: vec![IntegrationMode::StructuredStream, IntegrationMode::Sdk],
            capabilities: c,
        })
    }
    async fn start(&self, r: RunRequest) -> Result<RunHandle, AdapterError> {
        let Some((p, rest)) = r.argv.split_first() else {
            return Err(AdapterError::InvalidRequest("missing Cline command".into()));
        };
        let mut a = rest.to_vec();
        if !a.iter().any(|x| x.to_string_lossy() == "--json") {
            a.push("--json".into())
        }
        let mut spec = ProcessSpec::new(p.clone(), r.cwd);
        spec.args = a;
        self.runs
            .launch(r.run_id, HarnessId::Cline, spec, normalize_line)
            .await
    }
    async fn events(&self, r: &RunHandle) -> Result<EventStream, AdapterError> {
        self.runs.events(r.run_id)
    }
    async fn cancel(&self, r: &RunHandle) -> Result<(), AdapterError> {
        self.runs.cancel(r.run_id)
    }
}
pub fn normalize_line(r: Uuid, t: Uuid, s: &mut u64, line: &str) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![captured_stdout_event(r, t, s, HarnessId::Cline, line)];
    };
    if raw.get("type").and_then(Value::as_str) != Some("agent_event") {
        return vec![native(r, t, s, EventKind::Checkpoint, raw.clone(), raw)];
    }
    let e = raw.get("event").cloned().unwrap_or(Value::Null);
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "iteration_start" | "iteration_end" => vec![native(r, t, s, EventKind::Checkpoint, e, raw)],
        "content_start" => normalize_content_start(r, t, s, e, raw),
        "content_update" => vec![native(r, t, s, EventKind::Checkpoint, e, raw)],
        "content_end" => normalize_content_end(r, t, s, e, raw),
        "usage" => {
            let mut x = native(r, t, s, EventKind::ModelUsage, e.clone(), raw);
            x.usage = Some(TokenUsage {
                input_tokens: e.get("inputTokens").and_then(Value::as_u64),
                output_tokens: e.get("outputTokens").and_then(Value::as_u64),
                cached_input_tokens: e.get("cacheReadTokens").and_then(Value::as_u64),
                cache_write_input_tokens: e.get("cacheWriteTokens").and_then(Value::as_u64),
                reasoning_output_tokens: None,
            });
            if let Some(v) = e.get("cost").and_then(Value::as_f64) {
                x.cost = Some(Cost {
                    amount: v,
                    currency: "USD".into(),
                    basis: CostBasis::Reported,
                    price_table_version: None,
                })
            }
            vec![x]
        }
        "notice" => vec![native(
            r,
            t,
            s,
            if e.get("noticeType").and_then(Value::as_str) == Some("recovery") {
                EventKind::Retry
            } else {
                EventKind::Checkpoint
            },
            e,
            raw,
        )],
        "done" => vec![native(r, t, s, EventKind::RunCompleted, e, raw)],
        "error" => {
            let mut x = native(r, t, s, EventKind::RunFailed, e.clone(), raw);
            x.error = Some(ErrorInfo {
                message: e
                    .pointer("/error/message")
                    .or_else(|| e.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("Cline run failed")
                    .into(),
                code: e
                    .get("errorClass")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                recoverable: e.get("recoverable").and_then(Value::as_bool),
            });
            vec![x]
        }
        _ => vec![native(r, t, s, EventKind::Checkpoint, e, raw)],
    }
}
fn normalize_content_start(
    r: Uuid,
    t: Uuid,
    s: &mut u64,
    e: Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    match e.get("contentType").and_then(Value::as_str).unwrap_or("") {
        "text" => vec![native(r, t, s, EventKind::ModelResponse, e, raw)],
        "reasoning" => vec![native(r, t, s, EventKind::ReasoningStarted, e, raw)],
        "tool" => tool_start(r, t, s, e, raw),
        _ => vec![native(r, t, s, EventKind::Checkpoint, e, raw)],
    }
}
fn normalize_content_end(
    r: Uuid,
    t: Uuid,
    s: &mut u64,
    e: Value,
    raw: Value,
) -> Vec<EventEnvelope> {
    match e.get("contentType").and_then(Value::as_str).unwrap_or("") {
        "text" => vec![native(r, t, s, EventKind::ModelResponse, e, raw)],
        "reasoning" => vec![native(r, t, s, EventKind::ReasoningCompleted, e, raw)],
        "tool" => {
            let mut x = native(r, t, s, EventKind::ToolResult, e.clone(), raw.clone());
            if let Some(ms) = e.get("durationMs").and_then(Value::as_u64) {
                x.duration_ns = Some(ms.saturating_mul(1_000_000));
            }
            let mut out = vec![x];
            if is_shell(e.get("toolName").and_then(Value::as_str).unwrap_or("")) {
                out.push(native(
                    r,
                    t,
                    s,
                    EventKind::ShellOutput,
                    json!({"output":e.get("output"),"error":e.get("error")}),
                    raw,
                ));
            }
            out
        }
        _ => vec![native(r, t, s, EventKind::Checkpoint, e, raw)],
    }
}
fn tool_start(r: Uuid, t: Uuid, s: &mut u64, e: Value, raw: Value) -> Vec<EventEnvelope> {
    let name = e.get("toolName").and_then(Value::as_str).unwrap_or("");
    let input = e.get("input").cloned().unwrap_or(Value::Null);
    let mut out = vec![native(r, t, s, EventKind::ToolCall, e.clone(), raw.clone())];
    if is_shell(name) {
        let cmd = input.get("command").and_then(Value::as_str).unwrap_or("");
        let mut x = native(
            r,
            t,
            s,
            EventKind::ShellCommand,
            json!({"command":cmd}),
            raw,
        );
        x.command = Some(CommandInfo {
            program: "shell".into(),
            args: if cmd.is_empty() {
                vec![]
            } else {
                vec![cmd.into()]
            },
            cwd: None,
            exit_code: None,
        });
        out.push(x)
    } else if matches!(name, "read_file" | "Read") {
        file(&mut out, r, t, s, EventKind::FileRead, &input, raw)
    } else if matches!(name, "write_to_file" | "Write") {
        file(&mut out, r, t, s, EventKind::FileWrite, &input, raw)
    } else if matches!(name, "replace_in_file" | "apply_diff" | "Edit") {
        file(&mut out, r, t, s, EventKind::FilePatch, &input, raw)
    } else if name.contains("mcp") {
        out.push(native(r, t, s, EventKind::McpRequest, e, raw))
    }
    out
}
fn is_shell(n: &str) -> bool {
    matches!(n, "execute_command" | "Bash" | "bash")
}
fn file(
    o: &mut Vec<EventEnvelope>,
    r: Uuid,
    t: Uuid,
    s: &mut u64,
    k: EventKind,
    p: &Value,
    raw: Value,
) {
    let path = p
        .get("path")
        .or_else(|| p.get("file_path"))
        .and_then(Value::as_str);
    let mut e = native(r, t, s, k, json!({"path":path}), raw);
    let mut f = FilesystemImpact::default();
    if let Some(x) = path {
        if k == EventKind::FileRead {
            f.paths_read.push(x.into())
        } else {
            f.paths_written.push(x.into())
        }
    }
    e.filesystem_impact = Some(f);
    o.push(e)
}
fn native(r: Uuid, t: Uuid, s: &mut u64, k: EventKind, p: Value, raw: Value) -> EventEnvelope {
    native_harness_event(r, t, s, HarnessId::Cline, SOURCE, k, p, raw)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_and_tool_are_native() {
        let mut s = 0;
        let r = Uuid::new_v4();
        let t = Uuid::new_v4();
        let tool = normalize_line(
            r,
            t,
            &mut s,
            r#"{"type":"agent_event","event":{"type":"content_start","contentType":"tool","toolName":"execute_command","toolCallId":"1","input":{"command":"cargo test"}}}"#,
        );
        assert!(tool.iter().any(|x| x.kind == EventKind::ShellCommand));
        let u = normalize_line(
            r,
            t,
            &mut s,
            r#"{"type":"agent_event","event":{"type":"usage","inputTokens":10,"outputTokens":5,"totalInputTokens":10,"totalOutputTokens":5,"cost":0.001}}"#,
        );
        assert_eq!(u[0].cost.as_ref().map(|x| x.amount), Some(0.001));
    }
}
