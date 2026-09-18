use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityEvidence, CapabilityReport, Detection, EventStream,
    HarnessAdapter, ImportRequest, RunHandle, RunRequest,
};
use agenttrace_adapter_common::{
    StructuredRunRegistry, captured_stdout_event_mode, detect_binary, native_harness_event_mode,
};
use agenttrace_process::ProcessSpec;
use agenttrace_protocol::{
    Cost, CostBasis, ErrorInfo, EventEnvelope, EventKind, HarnessId, IntegrationMode, ModelInfo,
    ProvenanceLevel, TokenUsage,
};
use async_trait::async_trait;
use futures::stream;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

const ANALYTICS: &str = "aider --analytics-log JSONL";

#[derive(Clone, Default)]
pub struct AiderAdapter {
    runs: StructuredRunRegistry,
}

#[async_trait]
impl HarnessAdapter for AiderAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Aider
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        detect_binary(
            "aider",
            vec![IntegrationMode::ProcessWrap, IntegrationMode::LogImport],
        )
        .await
    }

    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut capabilities = BTreeMap::new();
        for capability in [
            Capability::TerminalOutput,
            Capability::Failures,
            Capability::Duration,
        ] {
            capabilities.insert(
                capability,
                CapabilityEvidence {
                    level: ProvenanceLevel::Native,
                    source: "AgentTrace process wrapper".into(),
                    notes: None,
                },
            );
        }
        for capability in [Capability::TokenUsage, Capability::Cost, Capability::RawEvents] {
            capabilities.insert(
                capability,
                CapabilityEvidence {
                    level: ProvenanceLevel::Native,
                    source: ANALYTICS.into(),
                    notes: Some(
                        "local analytics import; Aider deliberately excludes prompts/code from analytics"
                            .into(),
                    ),
                },
            );
        }
        capabilities.insert(
            Capability::ModelInteractions,
            CapabilityEvidence {
                level: ProvenanceLevel::Native,
                source: ANALYTICS.into(),
                notes: Some(
                    "model identity plus aggregate usage/cost metadata only; request/response content is unavailable from analytics"
                        .into(),
                ),
            },
        );
        for capability in [
            Capability::ToolCalls,
            Capability::ToolResults,
            Capability::McpActivity,
            Capability::Approvals,
            Capability::Subagents,
        ] {
            capabilities.insert(
                capability,
                CapabilityEvidence {
                    level: ProvenanceLevel::Unavailable,
                    source: ANALYTICS.into(),
                    notes: None,
                },
            );
        }
        Ok(CapabilityReport {
            harness: HarnessId::Aider,
            integration_modes: vec![IntegrationMode::ProcessWrap, IntegrationMode::LogImport],
            capabilities,
        })
    }

    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError> {
        let Some((program, args)) = request.argv.split_first() else {
            return Err(AdapterError::InvalidRequest("missing Aider command".into()));
        };
        let mut spec = ProcessSpec::new(program.clone(), request.cwd);
        spec.args = args.to_vec();
        self.runs
            .launch_mode(
                request.run_id,
                HarnessId::Aider,
                IntegrationMode::ProcessWrap,
                spec,
                normalize_process_line,
            )
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
        let mut sequence = 0;
        let mut events = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            events.extend(normalize_analytics(
                request.run_id,
                trace_id,
                &mut sequence,
                line,
            ));
        }
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn normalize_process_line(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    line: &str,
) -> Vec<EventEnvelope> {
    vec![captured_stdout_event_mode(
        run_id,
        trace_id,
        sequence,
        HarnessId::Aider,
        IntegrationMode::ProcessWrap,
        line,
    )]
}

pub fn normalize_analytics(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: &mut u64,
    line: &str,
) -> Vec<EventEnvelope> {
    let Ok(raw) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let name = raw
        .get("event")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "unknown".to_owned());
    let properties = raw.get("properties").cloned().unwrap_or_else(|| json!({}));
    let kind = if name == "launched" {
        EventKind::RunStarted
    } else if name == "message_send" {
        EventKind::ModelUsage
    } else if name.contains("error") || name.contains("exception") {
        EventKind::Error
    } else {
        EventKind::Checkpoint
    };
    let mut event = native_harness_event_mode(
        run_id,
        trace_id,
        sequence,
        HarnessId::Aider,
        IntegrationMode::LogImport,
        ANALYTICS,
        kind,
        json!({"event":name.clone(),"properties":properties}),
        raw,
    );

    if let Some(model) = event
        .payload
        .pointer("/properties/main_model")
        .and_then(Value::as_str)
    {
        event.model = Some(ModelInfo {
            id: model.into(),
            provider: None,
        });
    }

    if kind == EventKind::ModelUsage {
        let prompt_tokens = event
            .payload
            .pointer("/properties/prompt_tokens")
            .and_then(Value::as_u64);
        let completion_tokens = event
            .payload
            .pointer("/properties/completion_tokens")
            .and_then(Value::as_u64);
        if prompt_tokens.is_some() || completion_tokens.is_some() {
            event.usage = Some(TokenUsage {
                input_tokens: prompt_tokens,
                output_tokens: completion_tokens,
                cached_input_tokens: None,
                cache_write_input_tokens: None,
                reasoning_output_tokens: None,
            });
        }
        if let Some(amount) = event
            .payload
            .pointer("/properties/cost")
            .and_then(Value::as_f64)
        {
            event.cost = Some(Cost {
                amount,
                currency: "USD".into(),
                basis: CostBasis::Reported,
                price_table_version: None,
            });
        }
    }

    if kind == EventKind::Error {
        event.error = Some(ErrorInfo {
            message: name.clone(),
            code: Some(name),
            recoverable: None,
        });
    }
    vec![event]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytics_fixture_normalizes_usage_cost_and_errors() {
        let fixture = include_str!("../../../../fixtures/aider/analytics.jsonl");
        let mut sequence = 0;
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let events: Vec<_> = fixture
            .lines()
            .flat_map(|line| normalize_analytics(run_id, trace_id, &mut sequence, line))
            .collect();

        let usage = events
            .iter()
            .find(|event| event.kind == EventKind::ModelUsage)
            .expect("fixture should contain message_send usage");
        assert_eq!(
            usage.usage.as_ref().and_then(|usage| usage.input_tokens),
            Some(80)
        );
        assert_eq!(
            usage.usage.as_ref().and_then(|usage| usage.output_tokens),
            Some(43)
        );
        assert_eq!(usage.cost.as_ref().map(|cost| cost.amount), Some(0.00123));
        assert!(events.iter().any(|event| event.kind == EventKind::Error));
        assert!(events.iter().all(|event| event.raw_source.is_some()));
    }
}
