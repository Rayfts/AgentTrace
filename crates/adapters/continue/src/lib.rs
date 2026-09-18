use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    StructuredRunRegistry, captured_stdout_event_mode, detect_binary, native_harness_event_mode,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    ErrorInfo, EventEnvelope, EventKind, HarnessId, IntegrationMode, ProvenanceLevel,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString};
use uuid::Uuid;
const SOURCE: &str = "cn -p --format json";
#[derive(Clone, Default)]
pub struct ContinueAdapter {
    runs: StructuredRunRegistry,
}
#[async_trait]
impl HarnessAdapter for ContinueAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Continue
    }
    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary("cn", vec![IntegrationMode::ProcessWrap]).await
    }
    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut c = BTreeMap::new();
        for x in [
            Capability::FinalOutput,
            Capability::RawEvents,
            Capability::TerminalOutput,
            Capability::ContextChanges,
        ] {
            c.insert(x,CapabilityEvidence{level:ProvenanceLevel::Native,source:SOURCE.into(),notes:Some("current --format json is final structured output plus headless compaction status, not a full internal event stream".into())});
        }
        for x in [
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::ShellCommands,
            Capability::TokenUsage,
            Capability::Cost,
            Capability::Approvals,
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
            harness: HarnessId::Continue,
            integration_modes: vec![IntegrationMode::ProcessWrap],
            capabilities: c,
        })
    }
    async fn start(&self, r: RunRequest) -> Result<RunHandle, AdapterError> {
        let (p, a) = command(&r.argv)?;
        let mut spec = ProcessSpec::new(p, r.cwd);
        spec.args = a;
        self.runs
            .launch_mode(
                r.run_id,
                HarnessId::Continue,
                IntegrationMode::ProcessWrap,
                spec,
                normalize_line,
            )
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
        return Err(AdapterError::InvalidRequest(
            "missing Continue command".into(),
        ));
    };
    let mut a = rest.to_vec();
    if !a.iter().any(|x| x.to_string_lossy() == "-p") {
        return Err(AdapterError::InvalidRequest(
            "Continue tracing uses documented headless mode: `cn -p ...`".into(),
        ));
    }
    if let Some(i) = a.iter().position(|x| x.to_string_lossy() == "--format") {
        if a.get(i + 1).map(|x| x.to_string_lossy()) != Some("json".into()) {
            return Err(AdapterError::InvalidRequest(
                "Continue headless tracing requires --format json".into(),
            ));
        }
    } else {
        a.push("--format".into());
        a.push("json".into());
    }
    Ok((p.clone(), a))
}
pub fn normalize_line(r: Uuid, t: Uuid, s: &mut u64, line: &str) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![captured_stdout_event_mode(
            r,
            t,
            s,
            HarnessId::Continue,
            IntegrationMode::ProcessWrap,
            line,
        )];
    };
    let status = raw.get("status").and_then(Value::as_str);
    if status == Some("info")
        && raw
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|m| m.contains("compact"))
    {
        return vec![native_harness_event_mode(
            r,
            t,
            s,
            HarnessId::Continue,
            IntegrationMode::ProcessWrap,
            SOURCE,
            EventKind::ContextCompacted,
            raw.clone(),
            raw,
        )];
    }
    if status == Some("error") {
        let mut e = native_harness_event_mode(
            r,
            t,
            s,
            HarnessId::Continue,
            IntegrationMode::ProcessWrap,
            SOURCE,
            EventKind::RunFailed,
            raw.clone(),
            raw.clone(),
        );
        e.error = Some(ErrorInfo {
            message: raw
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Continue failed")
                .into(),
            code: None,
            recoverable: None,
        });
        return vec![e];
    }
    vec![
        native_harness_event_mode(
            r,
            t,
            s,
            HarnessId::Continue,
            IntegrationMode::ProcessWrap,
            SOURCE,
            EventKind::ModelResponse,
            json!({"response":raw.get("response").unwrap_or(&raw)}),
            raw.clone(),
        ),
        native_harness_event_mode(
            r,
            t,
            s,
            HarnessId::Continue,
            IntegrationMode::ProcessWrap,
            SOURCE,
            EventKind::RunCompleted,
            raw.clone(),
            raw,
        ),
    ]
}
