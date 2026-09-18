use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, ImportRequest, RunHandle, RunRequest,
};
use agenttrace_adapter_common::native_harness_event_mode;
use agenttrace_protocol::{EventEnvelope, EventKind, HarnessId, IntegrationMode, ProvenanceLevel};
use async_trait::async_trait;
use futures::stream;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;
const SOURCE: &str = "Roo Code persisted task JSON";
#[derive(Clone, Default)]
pub struct RooCodeAdapter;
#[async_trait]
impl HarnessAdapter for RooCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::RooCode
    }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        Ok(Detection{installed:false,executable:None,version:None,integration_modes:vec![IntegrationMode::SessionImport,IntegrationMode::FilesystemWatch],notes:vec!["upstream Roo Code repository is archived; AgentTrace supports persisted task import/watch rather than inventing a headless CLI".into()]})
    }
    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut c = BTreeMap::new();
        for x in [
            Capability::ModelInteractions,
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::ContextChanges,
            Capability::RawEvents,
        ] {
            c.insert(
                x,
                CapabilityEvidence {
                    level: ProvenanceLevel::Native,
                    source: SOURCE.into(),
                    notes: Some(
                        "only when present in api_conversation_history.json / ui_messages.json"
                            .into(),
                    ),
                },
            );
        }
        for x in [
            Capability::Duration,
            Capability::Latency,
            Capability::Approvals,
            Capability::Cost,
        ] {
            c.insert(
                x,
                CapabilityEvidence {
                    level: ProvenanceLevel::Unavailable,
                    source: SOURCE.into(),
                    notes: None,
                },
            );
        }
        Ok(CapabilityReport {
            harness: HarnessId::RooCode,
            integration_modes: vec![
                IntegrationMode::SessionImport,
                IntegrationMode::FilesystemWatch,
            ],
            capabilities: c,
        })
    }
    async fn start(&self, _: RunRequest) -> Result<RunHandle, AdapterError> {
        Err(AdapterError::Unsupported(IntegrationMode::ProcessWrap))
    }
    async fn events(&self, r: &RunHandle) -> Result<EventStream, AdapterError> {
        Err(AdapterError::RunNotActive(r.run_id))
    }
    async fn cancel(&self, r: &RunHandle) -> Result<(), AdapterError> {
        Err(AdapterError::RunNotActive(r.run_id))
    }
    async fn import(&self, r: ImportRequest) -> Result<EventStream, AdapterError> {
        let text = tokio::fs::read_to_string(&r.path).await?;
        let raw: Value =
            serde_json::from_str(&text).map_err(|e| AdapterError::Protocol(e.to_string()))?;
        let arr = raw.as_array().ok_or_else(|| {
            AdapterError::Protocol("Roo task history must be a JSON array".into())
        })?;
        let t = Uuid::new_v4();
        let mut s = 0;
        let mut out = Vec::new();
        for m in arr {
            out.extend(normalize_message(r.run_id, t, &mut s, m.clone()));
        }
        Ok(Box::pin(stream::iter(out.into_iter().map(Ok))))
    }
}
fn normalize_message(r: Uuid, t: Uuid, s: &mut u64, m: Value) -> Vec<EventEnvelope> {
    if m.get("type").and_then(Value::as_str) == Some("reasoning") {
        return vec![native(r, t, s, EventKind::ReasoningCompleted, m.clone(), m)];
    }
    let role = m.get("role").and_then(Value::as_str);
    if role.is_none() {
        return vec![native(r, t, s, EventKind::Checkpoint, m.clone(), m)];
    }
    let mut out = Vec::new();
    if let Some(content) = m.get("content").and_then(Value::as_array) {
        for b in content {
            let k = match b.get("type").and_then(Value::as_str) {
                Some("tool_use") => EventKind::ToolCall,
                Some("tool_result") => EventKind::ToolResult,
                Some("thinking") => EventKind::ReasoningCompleted,
                _ => {
                    if role == Some("assistant") {
                        EventKind::ModelResponse
                    } else {
                        EventKind::ModelRequest
                    }
                }
            };
            out.push(native(r, t, s, k, b.clone(), m.clone()));
        }
    } else {
        out.push(native(
            r,
            t,
            s,
            if role == Some("assistant") {
                EventKind::ModelResponse
            } else {
                EventKind::ModelRequest
            },
            json!({"content":m.get("content")}),
            m.clone(),
        ));
    }
    out
}
fn native(r: Uuid, t: Uuid, s: &mut u64, k: EventKind, p: Value, raw: Value) -> EventEnvelope {
    native_harness_event_mode(
        r,
        t,
        s,
        HarnessId::RooCode,
        IntegrationMode::SessionImport,
        SOURCE,
        k,
        p,
        raw,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_history_tool_is_preserved() {
        let mut s = 0;
        let e = normalize_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &mut s,
            json!({"role":"assistant","content":[{"type":"tool_use","id":"x","name":"read_file","input":{"path":"a"}}]}),
        );
        assert_eq!(e[0].kind, EventKind::ToolCall);
        assert!(e[0].raw_source.is_some());
    }
}
