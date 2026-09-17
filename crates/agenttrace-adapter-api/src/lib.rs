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
    fn omitted_capability_is_unavailable() {
        let report = CapabilityReport {
            harness: HarnessId::Aider,
            integration_modes: vec![IntegrationMode::ProcessWrap],
            capabilities: BTreeMap::new(),
        };
        assert_eq!(
            report.status(Capability::McpActivity),
            ProvenanceLevel::Unavailable
        );
    }
}
