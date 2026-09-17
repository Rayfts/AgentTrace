use std::path::PathBuf;

use agenttrace_adapter_api::{Capability, ImportRequest};
use agenttrace_protocol::{EventEnvelope, EventKind, ProvenanceLevel};
use agenttrace_redaction::{REDACTED, Redactor};
use agenttrace_registry::AdapterRegistry;
use agenttrace_storage::{ArtifactMetadata, RunSummary, TraceStore};
use futures::StreamExt;
use serde_json::{Map, Value, json};
use tauri::{Manager, State};
use uuid::Uuid;

struct AppState {
    store: TraceStore,
    database_path: PathBuf,
    registry: AdapterRegistry,
    redactor: Redactor,
}

#[tauri::command]
async fn list_runs(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<Value>, String> {
    state
        .store
        .list_runs(limit.unwrap_or(250).clamp(1, 1000))
        .await
        .map(|runs| runs.iter().map(summary_json).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_run(state: State<'_, AppState>, run_id: Uuid) -> Result<Value, String> {
    state
        .store
        .run_summary(run_id)
        .await
        .map_err(|error| error.to_string())?
        .map(|summary| summary_json(&summary))
        .ok_or_else(|| format!("run {run_id} was not found"))
}

#[tauri::command]
async fn get_run_events(
    state: State<'_, AppState>,
    run_id: Uuid,
    raw: Option<bool>,
) -> Result<Vec<EventEnvelope>, String> {
    let events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(|error| error.to_string())?;
    sanitize_events(events, raw.unwrap_or(false), &state.redactor)
}

#[tauri::command]
async fn get_run_stats(state: State<'_, AppState>, run_id: Uuid) -> Result<Value, String> {
    let events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(trace_stats(&events))
}

#[tauri::command]
async fn import_trace(
    state: State<'_, AppState>,
    harness: String,
    path: String,
) -> Result<Value, String> {
    let id = AdapterRegistry::parse(&harness)
        .ok_or_else(|| format!("unknown harness `{harness}`"))?;
    let adapter = state
        .registry
        .get(id)
        .ok_or_else(|| format!("harness `{harness}` is not registered"))?;
    let run_id = Uuid::new_v4();
    let mut stream = adapter
        .import(ImportRequest {
            run_id,
            path: PathBuf::from(path),
        })
        .await
        .map_err(|error| error.to_string())?;
    let mut imported_events = 0_u64;

    while let Some(event) = stream.next().await {
        let event = event.map_err(|error| error.to_string())?;
        let event = sanitize_event(event, &state.redactor)?;
        state
            .store
            .append_event(&event)
            .await
            .map_err(|error| error.to_string())?;
        imported_events = imported_events.saturating_add(1);
    }

    Ok(json!({
        "run_id": run_id,
        "harness": AdapterRegistry::canonical_name(id),
        "imported_events": imported_events,
    }))
}

#[tauri::command]
async fn compare_runs(
    state: State<'_, AppState>,
    left: Uuid,
    right: Uuid,
) -> Result<Value, String> {
    let left_events = state
        .store
        .load_run_events(left)
        .await
        .map_err(|error| error.to_string())?;
    let right_events = state
        .store
        .load_run_events(right)
        .await
        .map_err(|error| error.to_string())?;
    if left_events.is_empty()
        && state
            .store
            .run_summary(left)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err(format!("run {left} was not found"));
    }
    if right_events.is_empty()
        && state
            .store
            .run_summary(right)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err(format!("run {right} was not found"));
    }
    Ok(json!({
        "left": {"run_id": left, "stats": trace_stats(&left_events)},
        "right": {"run_id": right, "stats": trace_stats(&right_events)},
    }))
}

#[tauri::command]
async fn get_harness_capabilities(
    state: State<'_, AppState>,
    harness: String,
) -> Result<Value, String> {
    let id = AdapterRegistry::parse(&harness)
        .ok_or_else(|| format!("unknown harness `{harness}`"))?;
    let adapter = state
        .registry
        .get(id)
        .ok_or_else(|| format!("harness `{harness}` is not registered"))?;
    let report = adapter
        .capabilities()
        .await
        .map_err(|error| error.to_string())?;
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

#[tauri::command]
async fn export_run_sanitized(
    state: State<'_, AppState>,
    run_id: Uuid,
    raw: Option<bool>,
) -> Result<String, String> {
    let events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(|error| error.to_string())?;
    if events.is_empty()
        && state
            .store
            .run_summary(run_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err(format!("run {run_id} was not found"));
    }
    let events = sanitize_events(events, raw.unwrap_or(false), &state.redactor)?;
    let mut body = String::new();
    for event in events {
        body.push_str(&serde_json::to_string(&event).map_err(|error| error.to_string())?);
        body.push('\n');
    }
    Ok(body)
}

#[tauri::command]
async fn list_run_artifacts(
    state: State<'_, AppState>,
    run_id: Uuid,
) -> Result<Vec<Value>, String> {
    state
        .store
        .list_artifacts(run_id)
        .await
        .map(|artifacts| artifacts.iter().map(artifact_json).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn database_location(state: State<'_, AppState>) -> String {
    state.database_path.to_string_lossy().into_owned()
}

fn sanitize_event(event: EventEnvelope, redactor: &Redactor) -> Result<EventEnvelope, String> {
    let mut value = serde_json::to_value(event).map_err(|error| error.to_string())?;
    redact_sensitive_paths(&mut value, redactor);
    redactor.redact_json(&mut value);
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn sanitize_events(
    events: Vec<EventEnvelope>,
    include_raw: bool,
    redactor: &Redactor,
) -> Result<Vec<EventEnvelope>, String> {
    events
        .into_iter()
        .map(|event| {
            let mut event = sanitize_event(event, redactor)?;
            if !include_raw {
                event.raw_source = None;
            }
            Ok(event)
        })
        .collect()
}

fn redact_sensitive_paths(value: &mut Value, redactor: &Redactor) {
    match value {
        Value::String(text) if redactor.is_sensitive_path(text) => {
            *text = REDACTED.to_owned();
        }
        Value::Array(values) => {
            for value in values {
                redact_sensitive_paths(value, redactor);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                redact_sensitive_paths(value, redactor);
            }
        }
        _ => {}
    }
}

fn summary_json(summary: &RunSummary) -> Value {
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

fn artifact_json(artifact: &ArtifactMetadata) -> Value {
    json!({
        "artifact_id": artifact.artifact_id,
        "run_id": artifact.run_id,
        "event_id": artifact.event_id,
        "name": &artifact.name,
        "kind": &artifact.kind,
        "media_type": &artifact.media_type,
        "content_sha256": &artifact.content_sha256,
        "original_size": artifact.original_size,
        "compressed": artifact.compressed,
        "created_at": artifact.created_at,
    })
}

fn trace_stats(events: &[EventEnvelope]) -> Value {
    let mut kinds = std::collections::BTreeMap::<String, u64>::new();
    let mut provenance = std::collections::BTreeMap::<String, u64>::new();
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cached_input_tokens = 0_u64;
    let mut reasoning_output_tokens = 0_u64;
    let mut duration_ns = 0_u64;
    let mut costs = std::collections::BTreeMap::<String, f64>::new();

    for event in events {
        let kind = serde_json::to_value(event.kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        *kinds.entry(kind).or_default() += 1;
        *provenance
            .entry(provenance_name(event.provenance.level).into())
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
            "reasoning_output": reasoning_output_tokens
        },
        "reported_or_deterministic_cost_by_currency": costs,
        "sum_event_duration_ns": duration_ns
    })
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

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let database_path = match std::env::var_os("AGENTTRACE_DB") {
                Some(path) => PathBuf::from(path),
                None => {
                    let directory = app.path().app_data_dir()?;
                    std::fs::create_dir_all(&directory)?;
                    directory.join("agenttrace.db")
                }
            };
            let redactor = match std::env::var_os("AGENTTRACE_REDACTION_CONFIG") {
                Some(path) => {
                    let profile = std::fs::read_to_string(path)?;
                    Redactor::from_profile_json(&profile)?
                }
                None => Redactor::default(),
            };
            let store = tauri::async_runtime::block_on(TraceStore::open(&database_path))?;
            tauri::async_runtime::block_on(store.recover_interrupted_runs())?;
            app.manage(AppState {
                store,
                database_path,
                registry: AdapterRegistry::default(),
                redactor,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_runs,
            get_run,
            get_run_events,
            get_run_stats,
            import_trace,
            compare_runs,
            get_harness_capabilities,
            export_run_sanitized,
            list_run_artifacts,
            database_location
        ])
        .run(tauri::generate_context!())
        .expect("AgentTrace desktop runtime failed");
}
