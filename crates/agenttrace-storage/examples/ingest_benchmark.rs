use std::{error::Error, time::Instant};

use agenttrace_protocol::{
    EventEnvelope, EventKind, HarnessId, IntegrationMode, Provenance,
};
use agenttrace_storage::TraceStore;
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let count = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(10_000);
    if count == 0 {
        return Err("event count must be greater than zero".into());
    }

    let database = std::env::temp_dir().join(format!(
        "agenttrace-storage-benchmark-{}.db",
        Uuid::new_v4()
    ));
    let store = TraceStore::open(&database).await?;
    let run_id = Uuid::new_v4();
    let trace_id = Uuid::new_v4();

    let write_started = Instant::now();
    for sequence in 1..=count {
        let event = EventEnvelope::new(
            run_id,
            trace_id,
            sequence,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("storage-benchmark"),
            EventKind::Checkpoint,
            json!({
                "sequence": sequence,
                "message": "synthetic benchmark event",
            }),
        );
        store.append_event(&event).await?;
    }
    let write_elapsed = write_started.elapsed();

    let read_started = Instant::now();
    let events = store.load_run_events(run_id).await?;
    let read_elapsed = read_started.elapsed();

    println!("database={}", database.display());
    println!("events={}", events.len());
    println!("write_seconds={:.6}", write_elapsed.as_secs_f64());
    println!(
        "write_events_per_second={:.2}",
        count as f64 / write_elapsed.as_secs_f64()
    );
    println!("read_seconds={:.6}", read_elapsed.as_secs_f64());
    println!(
        "read_events_per_second={:.2}",
        events.len() as f64 / read_elapsed.as_secs_f64()
    );

    drop(store);
    for path in [
        database.clone(),
        database.with_extension("db-shm"),
        database.with_extension("db-wal"),
    ] {
        let _ = tokio::fs::remove_file(path).await;
    }

    Ok(())
}
