use std::sync::atomic::{AtomicU64, Ordering};

use agenttrace_process::ProcessEvent;
use agenttrace_protocol::{EventEnvelope, EventKind, HarnessId, IntegrationMode, Provenance};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug)]
pub struct Sequencer {
    next: AtomicU64,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Sequencer {
    pub const fn new(first: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
        }
    }

    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
}

pub fn normalize_process_event(
    run_id: Uuid,
    trace_id: Uuid,
    harness: HarnessId,
    sequencer: &Sequencer,
    event: ProcessEvent,
) -> EventEnvelope {
    let sequence = sequencer.next();
    let (kind, payload, duration_ns) = match event {
        ProcessEvent::Started { pid } => (EventKind::ProcessStarted, json!({"pid": pid}), None),
        ProcessEvent::Stdout { bytes } => (
            EventKind::ProcessStdout,
            json!({"encoding":"base64-or-utf8","bytes": bytes}),
            None,
        ),
        ProcessEvent::Stderr { bytes } => (
            EventKind::ProcessStderr,
            json!({"encoding":"base64-or-utf8","bytes": bytes}),
            None,
        ),
        ProcessEvent::Exited {
            code,
            success,
            duration,
        } => (
            EventKind::ProcessExited,
            json!({"exit_code": code, "success": success}),
            u64::try_from(duration.as_nanos()).ok(),
        ),
    };
    let mut envelope = EventEnvelope::new(
        run_id,
        trace_id,
        sequence,
        harness,
        IntegrationMode::ProcessWrap,
        Provenance::native("agenttrace-process"),
        kind,
        payload,
    );
    envelope.duration_ns = duration_ns;
    envelope
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_process_events_without_claiming_agent_telemetry() {
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let sequencer = Sequencer::default();
        let first = normalize_process_event(
            run_id,
            trace_id,
            HarnessId::Aider,
            &sequencer,
            ProcessEvent::Started { pid: Some(7) },
        );
        let second = normalize_process_event(
            run_id,
            trace_id,
            HarnessId::Aider,
            &sequencer,
            ProcessEvent::Stdout {
                bytes: b"hello".to_vec(),
            },
        );
        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.kind, EventKind::ProcessStdout);
    }
}
