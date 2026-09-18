use std::{collections::BTreeMap, sync::Arc};

use agenttrace_adapter_aider::AiderAdapter;
use agenttrace_adapter_api::{
    AdapterError, Capability, CapabilityReport, Detection, EventStream, HarnessAdapter,
    ImportRequest, RunHandle, RunRequest,
};
use agenttrace_adapter_claude_code::ClaudeCodeAdapter;
use agenttrace_adapter_cline::ClineAdapter;
use agenttrace_adapter_codex::CodexAdapter;
use agenttrace_adapter_continue::ContinueAdapter;
use agenttrace_adapter_gemini::GeminiAdapter;
use agenttrace_adapter_goose::GooseAdapter;
use agenttrace_adapter_opencode::OpenCodeAdapter;
use agenttrace_adapter_pi::PiAdapter;
use agenttrace_adapter_roo_code::RooCodeAdapter;
use agenttrace_protocol::{EventEnvelope, HarnessId, IntegrationMode};
use async_trait::async_trait;
use futures::StreamExt;

#[derive(Clone)]
pub struct AdapterRegistry {
    adapters: BTreeMap<HarnessId, Arc<dyn HarnessAdapter>>,
}

#[derive(Clone)]
struct RegisteredAdapter {
    inner: Arc<dyn HarnessAdapter>,
}

impl RegisteredAdapter {
    fn new<T>(adapter: T) -> Self
    where
        T: HarnessAdapter + 'static,
    {
        Self {
            inner: Arc::new(adapter),
        }
    }
}

#[async_trait]
impl HarnessAdapter for RegisteredAdapter {
    fn id(&self) -> HarnessId {
        self.inner.id()
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        let mut detection = self.inner.detect().await?;
        detection.integration_modes = implemented_modes(self.id(), detection.integration_modes);
        Ok(detection)
    }

    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError> {
        let mut report = self.inner.capabilities().await?;
        report.integration_modes = implemented_modes(self.id(), report.integration_modes);
        materialize_unavailable_capabilities(&mut report);
        Ok(report)
    }

    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError> {
        self.inner.start(request).await
    }

    async fn events(&self, run: &RunHandle) -> Result<EventStream, AdapterError> {
        let harness = self.id();
        let stream = self.inner.events(run).await?;
        Ok(Box::pin(stream.map(move |item| {
            item.map(|event| materialize_event_fields(harness, event))
        })))
    }

    async fn cancel(&self, run: &RunHandle) -> Result<(), AdapterError> {
        self.inner.cancel(run).await
    }

    async fn import(&self, request: ImportRequest) -> Result<EventStream, AdapterError> {
        let harness = self.id();
        let stream = self.inner.import(request).await?;
        Ok(Box::pin(stream.map(move |item| {
            item.map(|event| materialize_event_fields(harness, event))
        })))
    }
}

fn implemented_modes(harness: HarnessId, modes: Vec<IntegrationMode>) -> Vec<IntegrationMode> {
    modes
        .into_iter()
        .filter(|mode| match (harness, *mode) {
            (HarnessId::ClaudeCode, IntegrationMode::Hook)
            | (HarnessId::Pi, IntegrationMode::Rpc)
            | (HarnessId::Cline, IntegrationMode::Sdk)
            | (HarnessId::RooCode, IntegrationMode::FilesystemWatch) => false,
            _ => true,
        })
        .collect()
}

fn materialize_unavailable_capabilities(report: &mut CapabilityReport) {
    for capability in Capability::ALL {
        if report.capabilities.contains_key(&capability) {
            continue;
        }
        let evidence = report.evidence(capability);
        report.capabilities.insert(capability, evidence);
    }
}

fn materialize_event_fields(harness: HarnessId, mut event: EventEnvelope) -> EventEnvelope {
    if harness == HarnessId::ClaudeCode && event.latency_ns.is_none() {
        if let Some(api_ms) = event
            .attributes
            .get("duration_api_ms")
            .and_then(|value| value.as_u64())
        {
            event.latency_ns = Some(api_ms.saturating_mul(1_000_000));
        }
    }
    event
}

fn registered<T>(adapter: T) -> Arc<dyn HarnessAdapter>
where
    T: HarnessAdapter + 'static,
{
    Arc::new(RegisteredAdapter::new(adapter))
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        let adapters: [Arc<dyn HarnessAdapter>; 10] = [
            registered(CodexAdapter::default()),
            registered(ClaudeCodeAdapter::default()),
            registered(OpenCodeAdapter::default()),
            registered(PiAdapter::default()),
            registered(GeminiAdapter::default()),
            registered(AiderAdapter::default()),
            registered(GooseAdapter::default()),
            registered(ClineAdapter::default()),
            registered(RooCodeAdapter),
            registered(ContinueAdapter::default()),
        ];
        Self {
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.id(), adapter))
                .collect(),
        }
    }
}

impl AdapterRegistry {
    pub fn get(&self, harness: HarnessId) -> Option<Arc<dyn HarnessAdapter>> {
        self.adapters.get(&harness).cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = (HarnessId, Arc<dyn HarnessAdapter>)> + '_ {
        self.adapters
            .iter()
            .map(|(harness, adapter)| (*harness, Arc::clone(adapter)))
    }

    pub fn harnesses(&self) -> impl Iterator<Item = HarnessId> + '_ {
        self.adapters.keys().copied()
    }

    pub fn parse(input: &str) -> Option<HarnessId> {
        match input.trim().to_ascii_lowercase().as_str() {
            "codex" | "openai-codex" => Some(HarnessId::Codex),
            "claude" | "claude-code" => Some(HarnessId::ClaudeCode),
            "opencode" | "open-code" => Some(HarnessId::Opencode),
            "pi" => Some(HarnessId::Pi),
            "gemini" | "gemini-cli" => Some(HarnessId::Gemini),
            "aider" => Some(HarnessId::Aider),
            "goose" => Some(HarnessId::Goose),
            "cline" => Some(HarnessId::Cline),
            "roo" | "roo-code" => Some(HarnessId::RooCode),
            "continue" | "continue-cli" => Some(HarnessId::Continue),
            _ => None,
        }
    }

    pub fn canonical_name(harness: HarnessId) -> &'static str {
        match harness {
            HarnessId::Codex => "codex",
            HarnessId::ClaudeCode => "claude-code",
            HarnessId::Opencode => "opencode",
            HarnessId::Pi => "pi",
            HarnessId::Gemini => "gemini",
            HarnessId::Aider => "aider",
            HarnessId::Goose => "goose",
            HarnessId::Cline => "cline",
            HarnessId::RooCode => "roo-code",
            HarnessId::Continue => "continue",
            HarnessId::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use agenttrace_protocol::{EventEnvelope, EventKind, Provenance, ProvenanceLevel};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn registry_contains_all_initial_harnesses() {
        let registry = AdapterRegistry::default();
        assert_eq!(registry.harnesses().count(), 10);
        assert!(registry.get(HarnessId::Codex).is_some());
        assert!(registry.get(HarnessId::Continue).is_some());
    }

    #[test]
    fn aliases_resolve_without_guessing_unknown_names() {
        assert_eq!(AdapterRegistry::parse("gemini-cli"), Some(HarnessId::Gemini));
        assert_eq!(AdapterRegistry::parse("roo"), Some(HarnessId::RooCode));
        assert_eq!(AdapterRegistry::parse("made-up-agent"), None);
    }

    #[test]
    fn registry_does_not_advertise_researched_but_unimplemented_modes() {
        assert_eq!(
            implemented_modes(
                HarnessId::ClaudeCode,
                vec![IntegrationMode::StructuredStream, IntegrationMode::Hook],
            ),
            vec![IntegrationMode::StructuredStream]
        );
        assert_eq!(
            implemented_modes(HarnessId::Pi, vec![IntegrationMode::Rpc]),
            Vec::<IntegrationMode>::new()
        );
        assert_eq!(
            implemented_modes(HarnessId::Cline, vec![IntegrationMode::Sdk]),
            Vec::<IntegrationMode>::new()
        );
        assert_eq!(
            implemented_modes(
                HarnessId::RooCode,
                vec![
                    IntegrationMode::SessionImport,
                    IntegrationMode::FilesystemWatch,
                ],
            ),
            vec![IntegrationMode::SessionImport]
        );
    }

    #[test]
    fn registry_materializes_negative_capabilities() {
        let mut report = CapabilityReport {
            harness: HarnessId::Aider,
            integration_modes: vec![IntegrationMode::ProcessWrap],
            capabilities: BTreeMap::new(),
        };
        materialize_unavailable_capabilities(&mut report);
        assert_eq!(report.capabilities.len(), Capability::ALL.len());
        assert_eq!(
            report.status(Capability::McpActivity),
            ProvenanceLevel::Unavailable
        );
    }

    #[test]
    fn registry_promotes_verified_claude_api_latency() {
        let mut event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::ClaudeCode,
            IntegrationMode::StructuredStream,
            Provenance::native("claude fixture"),
            EventKind::RunCompleted,
            json!({}),
        );
        event
            .attributes
            .insert("duration_api_ms".into(), json!(900_u64));

        let event = materialize_event_fields(HarnessId::ClaudeCode, event);
        assert_eq!(event.latency_ns, Some(900_000_000));
    }

    #[test]
    fn registry_does_not_invent_latency_for_other_harnesses() {
        let mut event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunCompleted,
            json!({}),
        );
        event
            .attributes
            .insert("duration_api_ms".into(), json!(900_u64));

        let event = materialize_event_fields(HarnessId::Codex, event);
        assert_eq!(event.latency_ns, None);
    }
}
