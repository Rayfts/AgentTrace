use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, pin::Pin};

use agenttrace_protocol::{EventEnvelope, HarnessId, IntegrationMode, ProvenanceLevel};
use async_trait::async_trait;
use futures::Stream;
use thiserror::Error;
use uuid::Uuid;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<EventEnvelope, AdapterError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    ModelInteractions,
    ToolCalls,
    ToolResults,
    ShellCommands,
    TerminalOutput,
    FileReads,
    FileWrites,
    Patches,
    GitOperations,
    BrowserActivity,
    McpActivity,
    Approvals,
    Subagents,
    Retries,
    Failures,
    ContextChanges,
    Duration,
    Latency,
    TokenUsage,
    Cost,
    RawEvents,
    FinalOutput,
}

impl Capability {
    pub const ALL: [Self; 22] = [
        Self::ModelInteractions,
        Self::ToolCalls,
        Self::ToolResults,
        Self::ShellCommands,
        Self::TerminalOutput,
        Self::FileReads,
        Self::FileWrites,
        Self::Patches,
        Self::GitOperations,
        Self::BrowserActivity,
        Self::McpActivity,
        Self::Approvals,
        Self::Subagents,
        Self::Retries,
        Self::Failures,
        Self::ContextChanges,
        Self::Duration,
        Self::Latency,
        Self::TokenUsage,
        Self::Cost,
        Self::RawEvents,
        Self::FinalOutput,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityEvidence {
    pub level: ProvenanceLevel,
    pub source: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CapabilityReport {
    pub harness: HarnessId,
    pub integration_modes: Vec<IntegrationMode>,
    pub capabilities: BTreeMap<Capability, CapabilityEvidence>,
}

impl CapabilityReport {
    pub fn status(&self, capability: Capability) -> ProvenanceLevel {
        self.capabilities
            .get(&capability)
            .map(|evidence| evidence.level)
            .unwrap_or(ProvenanceLevel::Unavailable)
    }

    pub fn evidence(&self, capability: Capability) -> CapabilityEvidence {
        self.capabilities
            .get(&capability)
            .cloned()
            .unwrap_or_else(|| CapabilityEvidence {
                level: ProvenanceLevel::Unavailable,
                source: "adapter capability report".into(),
                notes: Some(
                    "the adapter does not currently expose or normalize this capability".into(),
                ),
            })
    }
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub installed: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub integration_modes: Vec<IntegrationMode>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub run_id: Uuid,
    pub cwd: PathBuf,
    pub argv: Vec<OsString>,
    pub integration_mode: Option<IntegrationMode>,
}

#[derive(Debug, Clone)]
pub struct RunHandle {
    pub run_id: Uuid,
    pub harness: HarnessId,
    pub integration_mode: IntegrationMode,
    pub native_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub run_id: Uuid,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("harness is not installed")]
    NotInstalled,
    #[error("integration mode is unsupported: {0:?}")]
    Unsupported(IntegrationMode),
    #[error("invalid adapter request: {0}")]
    InvalidRequest(String),
    #[error("adapter I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("adapter protocol error: {0}")]
    Protocol(String),
    #[error("run {0} is not active")]
    RunNotActive(Uuid),
}

#[async_trait]
pub trait HarnessAdapter: Send + Sync {
    fn id(&self) -> HarnessId;

    async fn detect(&self) -> Result<Detection, AdapterError>;

    async fn capabilities(&self) -> Result<CapabilityReport, AdapterError>;

    async fn start(&self, request: RunRequest) -> Result<RunHandle, AdapterError>;

    async fn events(&self, run: &RunHandle) -> Result<EventStream, AdapterError>;

    async fn cancel(&self, run: &RunHandle) -> Result<(), AdapterError>;

    async fn import(&self, request: ImportRequest) -> Result<EventStream, AdapterError> {
        let _ = request;
        Err(AdapterError::Unsupported(IntegrationMode::SessionImport))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_capability_is_explicitly_unavailable() {
        let report = CapabilityReport {
            harness: HarnessId::Aider,
            integration_modes: vec![IntegrationMode::ProcessWrap],
            capabilities: BTreeMap::new(),
        };
        assert_eq!(
            report.status(Capability::McpActivity),
            ProvenanceLevel::Unavailable
        );
        let evidence = report.evidence(Capability::McpActivity);
        assert_eq!(evidence.level, ProvenanceLevel::Unavailable);
        assert!(evidence.notes.is_some());
    }

    #[test]
    fn all_capability_categories_are_enumerated_once() {
        let unique: std::collections::BTreeSet<_> = Capability::ALL.into_iter().collect();
        assert_eq!(unique.len(), Capability::ALL.len());
    }
}
