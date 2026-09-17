use std::{collections::BTreeMap, net::SocketAddr};

use agenttrace_adapter_api::{Capability, HarnessAdapter};
use agenttrace_protocol::{EventEnvelope, EventKind, ProvenanceLevel};
use agenttrace_registry::AdapterRegistry;
use agenttrace_storage::{RunSummary, StorageError, TraceStore};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub allow_remote: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 4319)),
            allow_remote: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("refusing non-loopback bind {0}; set allow_remote explicitly to expose AgentTrace")]
    RemoteBindDenied(SocketAddr),
    #[error("server I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
struct AppState {
    store: TraceStore,
    registry: AdapterRegistry,
}

pub fn router(store: TraceStore, registry: AdapterRegistry) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/harnesses", get(harnesses))
        .route("/api/capabilities", get(capabilities))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/{run_id}", get(get_run))
        .route("/api/runs/{run_id}/events", get(get_events))
        .route("/api/runs/{run_id}/export", get(export_run))
        .route("/api/runs/{run_id}/stats", get(run_stats))
        .with_state(AppState { store, registry })
}

pub async fn serve(
    config: ServerConfig,
    store: TraceStore,
    registry: AdapterRegistry,
) -> Result<(), ServerError> {
    if !bind_allowed(config) {
        return Err(ServerError::RemoteBindDenied(config.bind));
    }
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, router(store, registry))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn bind_allowed(config: ServerConfig) -> bool {
    config.allow_remote || config.bind.ip().is_loopback()
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "agenttrace",
        "storage": "local-sqlite",
    }))
}

async fn harnesses(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let mut rows = Vec::new();
    for (harness, adapter) in state.registry.iter() {
        let detection = adapter.detect().await.map_err(ApiError::adapter)?;
        rows.push(json!({
            "harness": AdapterRegistry::canonical_name(harness),
            "installed": detection.installed,
            "executable": detection.executable,
            "version": detection.version,
            "integration_modes": detection.integration_modes,
            "notes": detection.notes,
        }));
    }
    Ok(Json(Value::Array(rows)))
}

#[derive(Debug, Deserialize)]
struct CapabilityQuery {
    harness: Option<String>,
}

async fn capabilities(
    State(state): State<AppState>,
    Query(query): Query<CapabilityQuery>,
) -> ApiResult<Json<Value>> {
    let adapters: Vec<_> = if let Some(name) = query.harness.as_deref() {
        let id = AdapterRegistry::parse(name)
            .ok_or_else(|| ApiError::bad_request(format!("unknown harness `{name}`")))?;
        vec![state
            .registry
            .get(id)
            .ok_or_else(|| ApiError::not_found(format!("harness `{name}` is not registered")))?]
    } else {
        state
            .registry
            .iter()
            .map(|(_, adapter)| adapter)
            .collect()
    };

    let mut reports = Vec::new();
    for adapter in adapters {
        reports.push(capability_report_json(adapter.as_ref()).await?);
    }
    Ok(Json(Value::Array(reports)))
}

#[derive(Debug, Deserialize)]
struct RunListQuery {
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    100
}

async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<RunListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = query.limit.clamp(1, 1000);
    let runs = state.store.list_runs(limit).await.map_err(ApiError::storage)?;
    Ok(Json(Value::Array(
        runs.iter().map(run_summary_json).collect(),
    )))
}

async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let summary = require_run(&state.store, run_id).await?;
    Ok(Json(run_summary_json(&summary)))
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    #[serde(default)]
    raw: bool,
}

async fn get_events(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Query(query): Query<EventQuery>,
) -> ApiResult<Json<Value>> {
    require_run(&state.store, run_id).await?;
    let mut events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(ApiError::storage)?;
    if !query.raw {
        for event in &mut events {
            event.raw_source = None;
        }
    }
    Ok(Json(json!({"run_id": run_id, "events": events})))
}

async fn export_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> ApiResult<Response> {
    require_run(&state.store, run_id).await?;
    let events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(ApiError::storage)?;
    let mut body = String::new();
    for event in events {
        body.push_str(
            &serde_json::to_string(&event)
                .map_err(|error| ApiError::internal(error.to_string()))?,
        );
        body.push('\n');
    }
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
        ),
        (
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=agenttrace-{run_id}.jsonl"))
                .map_err(|error| ApiError::internal(error.to_string()))?,
        ),
    ];
    Ok((headers, body).into_response())
}

async fn run_stats(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    require_run(&state.store, run_id).await?;
    let events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(ApiError::storage)?;
    Ok(Json(json!({
        "run_id": run_id,
        "stats": trace_stats(&events),
    })))
}

async fn require_run(store: &TraceStore, run_id: Uuid) -> ApiResult<RunSummary> {
    store
        .run_summary(run_id)
        .await
        .map_err(ApiError::storage)?
        .ok_or_else(|| ApiError::not_found(format!("run {run_id} was not found")))
}

async fn capability_report_json(adapter: &dyn HarnessAdapter) -> ApiResult<Value> {
    let report = adapter.capabilities().await.map_err(ApiError::adapter)?;
    let mut capabilities = Map::new();
    for (capability, evidence) in report.capabilities {
        capabilities.insert(
            capability_name(capability).to_owned(),
            json!({
                "provenance": evidence.level,
                "source": evidence.source,
                "notes": evidence.notes,
            }),
        );
    }
    Ok(json!({
        "harness": AdapterRegistry::canonical_name(report.harness),
        "integration_modes": report.integration_modes,
        "capabilities": capabilities,
    }))
}

fn run_summary_json(summary: &RunSummary) -> Value {
    json!({
        "run_id": summary.run_id,
        "trace_id": summary.trace_id,
        "harness": &summary.harness,
        "integration_mode": &summary.integration_mode,
        "status": &summary.status,
        "started_at": summary.started_at,
        "finished_at": &summary.finished_at,
        "last_sequence": summary.last_sequence,
        "event_count": summary.event_count,
    })
}

fn trace_stats(events: &[EventEnvelope]) -> Value {
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut provenance: BTreeMap<String, u64> = BTreeMap::new();
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cached_input_tokens = 0_u64;
    let mut reasoning_output_tokens = 0_u64;
    let mut duration_ns = 0_u64;
    let mut costs: BTreeMap<String, f64> = BTreeMap::new();

    for event in events {
        *kinds.entry(event_kind_name(event.kind)).or_default() += 1;
        *provenance
            .entry(provenance_name(event.provenance.level).to_owned())
            .or_default() += 1;
        if let Some(usage) = &event.usage {
            input_tokens = input_tokens.saturating_add(usage.input_tokens.unwrap_or_default());
            output_tokens = output_tokens.saturating_add(usage.output_tokens.unwrap_or_default());
            cached_input_tokens = cached_input_tokens
                .saturating_add(usage.cached_input_tokens.unwrap_or_default());
            reasoning_output_tokens = reasoning_output_tokens
                .saturating_add(usage.reasoning_output_tokens.unwrap_or_default());
        }
        duration_ns = duration_ns.saturating_add(event.duration_ns.unwrap_or_default());
        if let Some(cost) = &event.cost {
            *costs.entry(cost.currency.clone()).or_default() += cost.amount;
        }
    }

    json!({
        "event_count": events.len(),
        "event_kinds": kinds,
        "provenance": provenance,
        "tokens": {
            "input": input_tokens,
            "output": output_tokens,
            "cached_input": cached_input_tokens,
            "reasoning_output": reasoning_output_tokens,
        },
        "reported_or_deterministic_cost_by_currency": costs,
        "sum_event_duration_ns": duration_ns,
    })
}

fn event_kind_name(kind: EventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn provenance_name(level: ProvenanceLevel) -> &'static str {
    match level {
        ProvenanceLevel::Native => "native",
        ProvenanceLevel::Inferred => "inferred",
        ProvenanceLevel::Derived => "derived",
        ProvenanceLevel::Unavailable => "unavailable",
    }
}

fn capability_name(capability: Capability) -> &'static str {
    match capability {
        Capability::ModelInteractions => "model_interactions",
        Capability::ToolCalls => "tool_calls",
        Capability::ToolResults => "tool_results",
        Capability::ShellCommands => "shell_commands",
        Capability::TerminalOutput => "terminal_output",
        Capability::FileReads => "file_reads",
        Capability::FileWrites => "file_writes",
        Capability::Patches => "patches",
        Capability::GitOperations => "git_operations",
        Capability::McpActivity => "mcp_activity",
        Capability::Approvals => "approvals",
        Capability::Subagents => "subagents",
        Capability::Retries => "retries",
        Capability::Failures => "failures",
        Capability::ContextChanges => "context_changes",
        Capability::Duration => "duration",
        Capability::Latency => "latency",
        Capability::TokenUsage => "token_usage",
        Capability::Cost => "cost",
        Capability::RawEvents => "raw_events",
        Capability::FinalOutput => "final_output",
    }
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }

    fn internal(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }

    fn storage(error: StorageError) -> Self {
        Self::internal(error.to_string())
    }

    fn adapter(error: agenttrace_adapter_api::AdapterError) -> Self {
        Self::internal(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
                "status": self.status.as_u16(),
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_binding_requires_explicit_opt_in() {
        let remote = ServerConfig {
            bind: SocketAddr::from(([0, 0, 0, 0], 4319)),
            allow_remote: false,
        };
        assert!(!bind_allowed(remote));
        assert!(bind_allowed(ServerConfig {
            allow_remote: true,
            ..remote
        }));
        assert!(bind_allowed(ServerConfig::default()));
    }
}
