use std::path::PathBuf;

use agenttrace_protocol::{EventEnvelope, EventKind, ProvenanceLevel};
use agenttrace_storage::{RunSummary, TraceStore};
use serde_json::{Value, json};
use tauri::{Manager, State};
use uuid::Uuid;

struct AppState {
    store: TraceStore,
    database_path: PathBuf,
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
    let mut events = state
        .store
        .load_run_events(run_id)
        .await
        .map_err(|error| error.to_string())?;
    if !raw.unwrap_or(false) {
        for event in &mut events {
            event.raw_source = None;
        }
    }
    Ok(events)
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
fn database_location(state: State<'_, AppState>) -> String {
    state.database_path.to_string_lossy().into_owned()
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
        *provenance.entry(provenance_name(event.provenance.level).into()).or_default() += 1;
        if let Some(usage) = &event.usage {
            input_tokens = input_tokens.saturating_add(usage.input_tokens.unwrap_or_default());
            output_tokens = output_tokens.saturating_add(usage.output_tokens.unwrap_or_default());
            cached_input_tokens = cached_input_tokens.saturating_add(usage.cached_input_tokens.unwrap_or_default());
            reasoning_output_tokens = reasoning_output_tokens.saturating_add(usage.reasoning_output_tokens.unwrap_or_default());
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
            let store = tauri::async_runtime::block_on(TraceStore::open(&database_path))?;
            tauri::async_runtime::block_on(store.recover_interrupted_runs())?;
            app.manage(AppState {
                store,
                database_path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_runs,
            get_run,
            get_run_events,
            get_run_stats,
            database_location
        ])
        .run(tauri::generate_context!())
        .expect("AgentTrace desktop runtime failed");
}
