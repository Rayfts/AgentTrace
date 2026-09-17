use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    StructuredRunRegistry, captured_stdout_event, detect_binary, native_harness_event,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, HarnessId, IntegrationMode,
    ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString};
use uuid::Uuid;
const SOURCE: &str = "goose run --output-format stream-json";
#[derive(Clone, Default)]
pub struct GooseAdapter {
    runs: StructuredRunRegistry,
}
#[async_trait]
impl HarnessAdapter for GooseAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Goose
    }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary("goose", vec![IntegrationMode::StructuredStream]).await
    }
    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut c = BTreeMap::new();
        for x in [
            Capability::ModelInteractions,
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::McpActivity,
            Capability::Approvals,
            Capability::TokenUsage,
            Capability::Cost,
            Capability::Failures,
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
            harness: HarnessId::Goose,
            integration_modes: vec![IntegrationMode::StructuredStream],
            capabilities: c,
        })
    }
    async fn start(&self, r: RunRequest) -> Result<RunHandle, AdapterError> {
        let (p, a) = command(&r.argv)?;
        let mut spec = ProcessSpec::new(p, r.cwd);
        spec.args = a;
        self.runs
            .launch(r.run_id, HarnessId::Goose, spec, normalize_line)
            .await
    }
    async fn events(&self, r: &RunHandle) -> Result<EventStream, AdapterError> {
        self.runs.events(r.run_id)
    }
    async fn cancel(&self, r: &RunHandle) -> Result<(), AdapterError> {
        self.runs.cancel(r.run_id)
    }
}
fn command(argv: &[OsString]) -> Result<(OsString, Vec<OsString>), AdapterError> {
    let Some((p, rest)) = argv.split_first() else {
        return Err(AdapterError::InvalidRequest("missing Goose command".into()));
    };
    let mut a = rest.to_vec();
    if !a.iter().any(|x| x.to_string_lossy() == "run") {
        return Err(AdapterError::InvalidRequest(
            "structured Goose tracing requires `goose run ...`".into(),
        ));
    }
    if let Some(i) = a
        .iter()
        .position(|x| x.to_string_lossy() == "--output-format")
    {
        if a.get(i + 1).map(|x| x.to_string_lossy()) != Some("stream-json".into()) {
            return Err(AdapterError::InvalidRequest(
                "Goose tracing requires --output-format stream-json".into(),
            ));
        }
    } else {
        a.push("--output-format".into());
        a.push("stream-json".into());
    }
    Ok((p.clone(), a))
}
pub fn normalize_line(r: Uuid, t: Uuid, s: &mut u64, line: &str) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![captured_stdout_event(r, t, s, HarnessId::Goose, line)];
    };
    match raw.get("type").and_then(Value::as_str).unwrap_or("unknown") {
        "message" => normalize_message(r, t, s, raw),
        "notification" => vec![native(r, t, s, EventKind::Checkpoint, raw.clone(), raw)],
        "error" => {
            let mut e = native(r, t, s, EventKind::Error, raw.clone(), raw.clone());
            e.error = Some(ErrorInfo {
                message: raw
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Goose error")
                    .into(),
                code: None,
                recoverable: None,
            });
            vec![e]
        }
        "complete" => {
            let mut u = native(r, t, s, EventKind::ModelUsage, raw.clone(), raw.clone());
            u.usage = Some(TokenUsage {
                input_tokens: raw.get("input_tokens").and_then(Value::as_u64),
                output_tokens: raw.get("output_tokens").and_then(Value::as_u64),
                cached_input_tokens: raw.get("cache_read_input_tokens").and_then(Value::as_u64),
                cache_write_input_tokens: raw
                    .get("cache_write_input_tokens")
                    .and_then(Value::as_u64),
                reasoning_output_tokens: None,
            });
            if let Some(v) = raw.get("cost_usd").and_then(Value::as_f64) {
                u.cost = Some(Cost {
                    amount: v,
                    currency: "USD".into(),
                    basis: CostBasis::Reported,
                    price_table_version: None,
                })
            }
            vec![
                u,
                native(r, t, s, EventKind::RunCompleted, raw.clone(), raw),
            ]
        }
        _ => vec![native(r, t, s, EventKind::Checkpoint, raw.clone(), raw)],
    }
}
fn normalize_message(r: Uuid, t: Uuid, s: &mut u64, raw: Value) -> Vec<EventEnvelope> {
    let m = raw.get("message").cloned().unwrap_or(Value::Null);
    let role = m.get("role").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();
    if let Some(content) = m.get("content").and_then(Value::as_array) {
        for b in content {
            match b.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => out.push(native(
                    r,
                    t,
                    s,
                    if role == "assistant" {
                        EventKind::ModelResponse
                    } else {
                        EventKind::ModelRequest
                    },
                    b.clone(),
                    raw.clone(),
                )),
                "thinking" | "redactedThinking" => out.push(native(
                    r,
                    t,
                    s,
                    EventKind::ReasoningCompleted,
                    b.clone(),
                    raw.clone(),
                )),
                "toolRequest" => {
                    out.push(native(r, t, s, EventKind::ToolCall, b.clone(), raw.clone()))
                }
                "toolResponse" => out.push(native(
                    r,
                    t,
                    s,
                    EventKind::ToolResult,
                    b.clone(),
                    raw.clone(),
                )),
                "toolConfirmationRequest" | "actionRequired" => out.push(native(
                    r,
                    t,
                    s,
                    EventKind::ApprovalRequested,
                    b.clone(),
                    raw.clone(),
                )),
                "error" => out.push(native(r, t, s, EventKind::Error, b.clone(), raw.clone())),
                _ => out.push(native(
                    r,
                    t,
                    s,
                    EventKind::Checkpoint,
                    b.clone(),
                    raw.clone(),
                )),
            }
        }
    }
    out
}
fn native(r: Uuid, t: Uuid, s: &mut u64, k: EventKind, p: Value, raw: Value) -> EventEnvelope {
    native_harness_event(r, t, s, HarnessId::Goose, SOURCE, k, p, raw)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_has_reported_cost() {
        let mut s = 0;
        let e = normalize_line(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &mut s,
            r#"{"type":"complete","total_tokens":20,"input_tokens":12,"output_tokens":8,"cost_usd":0.004}"#,
        );
        assert_eq!(e[0].cost.as_ref().map(|x| x.amount), Some(0.004));
        assert_eq!(e[1].kind, EventKind::RunCompleted);
    }
}
