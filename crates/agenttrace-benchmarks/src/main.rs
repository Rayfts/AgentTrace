use std::{
    alloc::{GlobalAlloc, Layout, System},
    error::Error,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use agenttrace_adapter_codex::normalize_line as normalize_codex_line;
use agenttrace_protocol::{
    CommandInfo, EventEnvelope, EventKind, FilesystemImpact, HarnessId, IntegrationMode,
    Provenance, SCHEMA_VERSION,
};
use agenttrace_storage::TraceStore;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use serde_json::json;
use tempfile::tempdir;
use uuid::Uuid;

struct CountingAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            account_alloc(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        account_dealloc(layout.size());
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            if new_size >= layout.size() {
                account_alloc(new_size - layout.size());
            } else {
                account_dealloc(layout.size() - new_size);
            }
        }
        new_pointer
    }
}

#[derive(Debug, Deserialize)]
struct LargeTraceFixture {
    name: String,
    schema_version: u16,
    event_count: u64,
    payload_bytes: usize,
    normalization_iterations: u64,
    harness: String,
    integration_mode: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let fixture: LargeTraceFixture = serde_json::from_str(include_str!(
        "../../../fixtures/benchmarks/large-trace.json"
    ))?;
    validate_fixture(&fixture)?;

    let directory = tempdir()?;
    let database_path = directory.path().join("agenttrace-benchmark.db");
    let store = TraceStore::open(&database_path).await?;
    let run_id = Uuid::from_u128(1);
    let trace_id = Uuid::from_u128(2);
    let base_time = DateTime::parse_from_rfc3339("2026-09-17T00:00:00Z")?.with_timezone(&Utc);
    let filler = "x".repeat(fixture.payload_bytes);

    let write_started = Instant::now();
    for sequence in 1..=fixture.event_count {
        let event = benchmark_event(
            run_id,
            trace_id,
            sequence,
            fixture.event_count,
            base_time,
            &filler,
        );
        store.append_event(&event).await?;
    }
    let write_elapsed = write_started.elapsed();

    let load_baseline = reset_peak_to_live();
    let load_started = Instant::now();
    let events = store.load_run_events(run_id).await?;
    let load_elapsed = load_started.elapsed();
    let load_heap_peak_delta = peak_delta(load_baseline);

    let export_baseline = reset_peak_to_live();
    let export_started = Instant::now();
    let mut exported = Vec::new();
    for event in &events {
        serde_json::to_writer(&mut exported, event)?;
        exported.push(b'\n');
    }
    let export_elapsed = export_started.elapsed();
    let export_heap_peak_delta = peak_delta(export_baseline);

    let normalization_fixture = include_str!("../../../fixtures/codex/exec.jsonl");
    let normalization_lines: Vec<&str> = normalization_fixture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if normalization_lines.is_empty() {
        return Err("Codex normalization fixture is empty".into());
    }
    let normalization_started = Instant::now();
    let mut normalized_sequence = 0_u64;
    let mut normalized_events = 0_u64;
    for index in 0..fixture.normalization_iterations {
        let line = normalization_lines[index as usize % normalization_lines.len()];
        normalized_events += normalize_codex_line(
            run_id,
            trace_id,
            &mut normalized_sequence,
            line,
        )
        .len() as u64;
    }
    let normalization_elapsed = normalization_started.elapsed();

    let database_bytes = tokio::fs::metadata(&database_path).await?.len();
    let report = json!({
        "fixture": {
            "name": fixture.name,
            "schema_version": fixture.schema_version,
            "event_count": fixture.event_count,
            "payload_bytes": fixture.payload_bytes,
            "normalization_iterations": fixture.normalization_iterations,
            "harness": fixture.harness,
            "integration_mode": fixture.integration_mode,
        },
        "ingestion": {
            "seconds": write_elapsed.as_secs_f64(),
            "events_per_second": rate(fixture.event_count, write_elapsed),
            "database_bytes": database_bytes,
        },
        "large_trace_load": {
            "seconds": load_elapsed.as_secs_f64(),
            "events": events.len(),
            "events_per_second": rate(events.len() as u64, load_elapsed),
            "rust_heap_peak_delta_bytes": load_heap_peak_delta,
        },
        "jsonl_export": {
            "seconds": export_elapsed.as_secs_f64(),
            "bytes": exported.len(),
            "events_per_second": rate(events.len() as u64, export_elapsed),
            "rust_heap_peak_delta_bytes": export_heap_peak_delta,
        },
        "codex_normalization": {
            "seconds": normalization_elapsed.as_secs_f64(),
            "input_records": fixture.normalization_iterations,
            "normalized_events": normalized_events,
            "records_per_second": rate(fixture.normalization_iterations, normalization_elapsed),
        },
        "memory_metric": "rust_heap_peak_delta_bytes tracks allocations made through Rust's System allocator during the measured phase; it is not total process RSS and does not include SQLite/native-library allocations"
    });

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn validate_fixture(fixture: &LargeTraceFixture) -> Result<(), Box<dyn Error>> {
    if fixture.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "fixture schema {} does not match AgentTrace schema {}",
            fixture.schema_version, SCHEMA_VERSION
        )
        .into());
    }
    if fixture.event_count < 2 {
        return Err("large trace fixture must contain at least two events".into());
    }
    if fixture.normalization_iterations == 0 {
        return Err("normalization_iterations must be greater than zero".into());
    }
    if fixture.harness != "codex" || fixture.integration_mode != "structured_stream" {
        return Err("benchmark fixture currently expects codex/structured_stream".into());
    }
    Ok(())
}

fn benchmark_event(
    run_id: Uuid,
    trace_id: Uuid,
    sequence: u64,
    event_count: u64,
    base_time: DateTime<Utc>,
    filler: &str,
) -> EventEnvelope {
    let kind = if sequence == 1 {
        EventKind::RunStarted
    } else if sequence == event_count {
        EventKind::RunCompleted
    } else {
        const KINDS: [EventKind; 10] = [
            EventKind::Checkpoint,
            EventKind::ModelResponse,
            EventKind::ToolCall,
            EventKind::ToolResult,
            EventKind::ShellCommand,
            EventKind::ShellOutput,
            EventKind::FileRead,
            EventKind::FileWrite,
            EventKind::McpRequest,
            EventKind::McpResponse,
        ];
        KINDS[(sequence as usize - 2) % KINDS.len()]
    };

    let mut event = EventEnvelope::new(
        run_id,
        trace_id,
        sequence,
        HarnessId::Codex,
        IntegrationMode::StructuredStream,
        Provenance::native("agenttrace-benchmark-fixture"),
        kind,
        json!({
            "fixture": true,
            "sequence": sequence,
            "payload": filler,
        }),
    );
    event.event_id = Uuid::from_u128(0x1000_0000_0000_0000_0000_0000_0000_0000_u128 + sequence as u128);
    event.timestamp = base_time + TimeDelta::milliseconds(sequence as i64);

    if kind == EventKind::ShellCommand {
        event.command = Some(CommandInfo {
            program: "shell".into(),
            args: vec![format!("printf benchmark-{}", sequence % 128)],
            cwd: None,
            exit_code: None,
        });
    }
    if matches!(kind, EventKind::FileRead | EventKind::FileWrite) {
        let path = format!("src/fixture_{:03}.rs", sequence % 256);
        let mut impact = FilesystemImpact::default();
        if kind == EventKind::FileRead {
            impact.paths_read.push(path);
        } else {
            impact.paths_written.push(path);
        }
        event.filesystem_impact = Some(impact);
    }
    event
}

fn rate(count: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds == 0.0 {
        f64::INFINITY
    } else {
        count as f64 / seconds
    }
}

fn account_alloc(bytes: usize) {
    let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    let mut peak = PEAK_BYTES.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_BYTES.compare_exchange_weak(
            peak,
            live,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => peak = observed,
        }
    }
}

fn account_dealloc(bytes: usize) {
    LIVE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
}

fn reset_peak_to_live() -> usize {
    let live = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(live, Ordering::Relaxed);
    live
}

fn peak_delta(baseline: usize) -> usize {
    PEAK_BYTES.load(Ordering::Relaxed).saturating_sub(baseline)
}
