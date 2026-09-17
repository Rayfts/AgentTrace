use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    Codex,
    ClaudeCode,
    Opencode,
    Pi,
    Gemini,
    Aider,
    Goose,
    Cline,
    RooCode,
    Continue,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    ProcessWrap,
    StructuredStream,
    Hook,
    SessionImport,
    LogImport,
    Extension,
    McpProxy,
    FilesystemWatch,
    Rpc,
    Sdk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceLevel {
    Native,
    Inferred,
    Derived,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub level: ProvenanceLevel,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Provenance {
    pub fn native(source: impl Into<String>) -> Self {
        Self {
            level: ProvenanceLevel::Native,
            source: source.into(),
            native_fields: Vec::new(),
            unavailable_fields: Vec::new(),
            notes: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    #[serde(rename = "run.started")]
    RunStarted,
    #[serde(rename = "run.completed")]
    RunCompleted,
    #[serde(rename = "run.failed")]
    RunFailed,
    #[serde(rename = "process.started")]
    ProcessStarted,
    #[serde(rename = "process.stdout")]
    ProcessStdout,
    #[serde(rename = "process.stderr")]
    ProcessStderr,
    #[serde(rename = "process.exited")]
    ProcessExited,
    #[serde(rename = "model.request")]
    ModelRequest,
    #[serde(rename = "model.response")]
    ModelResponse,
    #[serde(rename = "model.usage")]
    ModelUsage,
    #[serde(rename = "reasoning.started")]
    ReasoningStarted,
    #[serde(rename = "reasoning.completed")]
    ReasoningCompleted,
    #[serde(rename = "tool.call")]
    ToolCall,
    #[serde(rename = "tool.result")]
    ToolResult,
    #[serde(rename = "shell.command")]
    ShellCommand,
    #[serde(rename = "shell.output")]
    ShellOutput,
    #[serde(rename = "file.read")]
    FileRead,
    #[serde(rename = "file.write")]
    FileWrite,
    #[serde(rename = "file.create")]
    FileCreate,
    #[serde(rename = "file.delete")]
    FileDelete,
    #[serde(rename = "file.patch")]
    FilePatch,
    #[serde(rename = "git.operation")]
    GitOperation,
    #[serde(rename = "mcp.request")]
    McpRequest,
    #[serde(rename = "mcp.response")]
    McpResponse,
    #[serde(rename = "approval.requested")]
    ApprovalRequested,
    #[serde(rename = "approval.resolved")]
    ApprovalResolved,
    #[serde(rename = "subagent.started")]
    SubagentStarted,
    #[serde(rename = "subagent.completed")]
    SubagentCompleted,
    #[serde(rename = "context.added")]
    ContextAdded,
    #[serde(rename = "context.removed")]
    ContextRemoved,
    #[serde(rename = "context.compacted")]
    ContextCompacted,
    Retry,
    Checkpoint,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawSource {
    pub source: String,
    pub media_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
    pub basis: CostBasis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_table_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    Reported,
    DeterministicCalculation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FilesystemImpact {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_read: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_written: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_created: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_deleted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInfo {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u16,
    pub event_id: Uuid,
    pub run_id: Uuid,
    pub trace_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<Uuid>,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monotonic_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ns: Option<u64>,
    pub harness: HarnessId,
    pub integration_mode: IntegrationMode,
    pub provenance: Provenance,
    pub kind: EventKind,
    #[serde(default)]
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_source: Option<RawSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem_impact: Option<FilesystemImpact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
}

impl EventEnvelope {
    pub fn new(
        run_id: Uuid,
        trace_id: Uuid,
        sequence: u64,
        harness: HarnessId,
        integration_mode: IntegrationMode,
        provenance: Provenance,
        kind: EventKind,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::new_v4(),
            run_id,
            trace_id,
            span_id: None,
            parent_span_id: None,
            sequence,
            timestamp: Utc::now(),
            monotonic_ns: None,
            duration_ns: None,
            harness,
            integration_mode,
            provenance,
            kind,
            payload,
            raw_source: None,
            model: None,
            usage: None,
            cost: None,
            filesystem_impact: None,
            command: None,
            error: None,
            attributes: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_kind_uses_stable_wire_name() {
        assert_eq!(
            serde_json::to_string(&EventKind::ToolCall).unwrap(),
            "\"tool.call\""
        );
        assert_eq!(
            serde_json::to_string(&EventKind::ContextCompacted).unwrap(),
            "\"context.compacted\""
        );
    }

    #[test]
    fn preserves_explicit_unavailable_fields() {
        let provenance = Provenance {
            level: ProvenanceLevel::Native,
            source: "fixture".into(),
            native_fields: vec!["payload.command".into()],
            unavailable_fields: vec!["usage.reasoning_output_tokens".into()],
            notes: None,
        };
        let event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            provenance,
            EventKind::ShellCommand,
            json!({"command":"cargo test"}),
        );
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            decoded.provenance.unavailable_fields,
            vec!["usage.reasoning_output_tokens"]
        );
        assert_eq!(decoded.schema_version, SCHEMA_VERSION);
    }
}
