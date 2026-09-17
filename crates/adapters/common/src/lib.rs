use std::{collections::HashMap, io::ErrorKind, sync::{Arc, Mutex}};

use agenttrace_adapter_api::{AdapterError, Detection, EventStream, RunHandle};
use agenttrace_process::{ProcessEvent, ProcessSpec, SupervisedProcess};
use agenttrace_protocol::{EventEnvelope, EventKind, HarnessId, IntegrationMode, Provenance, RawSource};
use futures::stream;
use serde_json::{json, Value};
use tokio::{process::Command, sync::{mpsc, oneshot}};
use uuid::Uuid;

pub type LineNormalizer = fn(Uuid, Uuid, &mut u64, &str) -> Vec<EventEnvelope>;

struct ActiveRun {
    events: Option<mpsc::Receiver<Result<EventEnvelope, AdapterError>>>,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct StructuredRunRegistry { runs: Arc<Mutex<HashMap<Uuid, ActiveRun>>> }

impl StructuredRunRegistry {
    pub async fn launch(&self, run_id: Uuid, harness: HarnessId, spec: ProcessSpec, normalizer: LineNormalizer) -> Result<RunHandle, AdapterError> {
        self.launch_mode(run_id, harness, IntegrationMode::StructuredStream, spec, normalizer).await
    }

    pub async fn launch_mode(&self, run_id: Uuid, harness: HarnessId, integration_mode: IntegrationMode, spec: ProcessSpec, normalizer: LineNormalizer) -> Result<RunHandle, AdapterError> {
        {
            let runs = self.runs.lock().map_err(|_| AdapterError::Protocol("run registry lock poisoned".into()))?;
            if runs.contains_key(&run_id) { return Err(AdapterError::InvalidRequest(format!("run {run_id} is already active"))); }
        }
        let mut process = SupervisedProcess::spawn(spec).await.map_err(|error| AdapterError::Protocol(error.to_string()))?;
        let trace_id = Uuid::new_v4();
        let (event_tx, event_rx) = mpsc::channel(512);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.runs.lock().map_err(|_| AdapterError::Protocol("run registry lock poisoned".into()))?.insert(run_id, ActiveRun { events: Some(event_rx), cancel: Some(cancel_tx) });

        tokio::spawn(async move {
            let mut sequence = 0_u64;
            let mut stdout = Vec::<u8>::new();
            let mut cancellation_seen = false;
            loop {
                tokio::select! {
                    _ = &mut cancel_rx, if !cancellation_seen => { cancellation_seen = true; let _ = process.cancel(); }
                    process_event = process.next_event() => {
                        let Some(process_event) = process_event else { break; };
                        match process_event {
                            Ok(ProcessEvent::Started { pid }) => {
                                let event = process_envelope(run_id, trace_id, &mut sequence, harness, integration_mode, EventKind::ProcessStarted, json!({"pid":pid}));
                                if event_tx.send(Ok(event)).await.is_err() { break; }
                            }
                            Ok(ProcessEvent::Stdout { bytes }) => {
                                stdout.extend_from_slice(&bytes);
                                while let Some(newline) = stdout.iter().position(|byte| *byte == b'\n') {
                                    let line = stdout.drain(..=newline).collect::<Vec<_>>();
                                    let line = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)]);
                                    for mut event in normalizer(run_id, trace_id, &mut sequence, line.trim_end_matches('\r')) {
                                        event.integration_mode = integration_mode;
                                        if event_tx.send(Ok(event)).await.is_err() { return; }
                                    }
                                }
                            }
                            Ok(ProcessEvent::Stderr { bytes }) => {
                                let text = String::from_utf8_lossy(&bytes).into_owned();
                                let event = process_envelope(run_id, trace_id, &mut sequence, harness, integration_mode, EventKind::ProcessStderr, json!({"text":text}));
                                if event_tx.send(Ok(event)).await.is_err() { break; }
                            }
                            Ok(ProcessEvent::Exited { code, success, duration }) => {
                                if !stdout.is_empty() {
                                    let line = String::from_utf8_lossy(&stdout).into_owned();
                                    for mut event in normalizer(run_id, trace_id, &mut sequence, line.trim_end_matches(['\r','\n'])) {
                                        event.integration_mode = integration_mode;
                                        if event_tx.send(Ok(event)).await.is_err() { return; }
                                    }
                                }
                                let mut event = process_envelope(run_id, trace_id, &mut sequence, harness, integration_mode, EventKind::ProcessExited, json!({"code":code,"success":success}));
                                event.duration_ns = Some(duration.as_nanos().min(u64::MAX as u128) as u64);
                                let _ = event_tx.send(Ok(event)).await; break;
                            }
                            Err(error) => { let _ = event_tx.send(Err(AdapterError::Protocol(format!("process supervisor: {error}")))).await; break; }
                        }
                    }
                }
            }
        });
        Ok(RunHandle { run_id, harness, integration_mode, native_id: None })
    }

    pub fn events(&self, run_id: Uuid) -> Result<EventStream, AdapterError> {
        let receiver = self.runs.lock().map_err(|_| AdapterError::Protocol("run registry lock poisoned".into()))?.get_mut(&run_id).ok_or(AdapterError::RunNotActive(run_id))?.events.take().ok_or_else(|| AdapterError::InvalidRequest(format!("events for run {run_id} were already consumed")))?;
        Ok(Box::pin(stream::unfold(receiver, |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) })))
    }
    pub fn cancel(&self, run_id: Uuid) -> Result<(), AdapterError> {
        let sender = self.runs.lock().map_err(|_| AdapterError::Protocol("run registry lock poisoned".into()))?.get_mut(&run_id).ok_or(AdapterError::RunNotActive(run_id))?.cancel.take().ok_or(AdapterError::RunNotActive(run_id))?;
        sender.send(()).map_err(|_| AdapterError::RunNotActive(run_id))
    }
}

pub async fn detect_binary(binary: &str, integration_modes: Vec<IntegrationMode>) -> Result<Detection, AdapterError> {
    match Command::new(binary).arg("--version").output().await {
        Ok(output) => { let version = String::from_utf8_lossy(if output.stdout.is_empty(){&output.stderr}else{&output.stdout}).trim().to_owned(); Ok(Detection { installed: output.status.success(), executable: output.status.success().then(|| binary.into()), version: (!version.is_empty()).then_some(version), integration_modes, notes: Vec::new() }) }
        Err(error) if error.kind()==ErrorKind::NotFound => Ok(Detection { installed:false, executable:None, version:None, integration_modes, notes:vec![format!("{binary} was not found on PATH")] }),
        Err(error) => Err(AdapterError::Io(error)),
    }
}

pub fn native_harness_event(run_id:Uuid,trace_id:Uuid,sequence:&mut u64,harness:HarnessId,source:&str,kind:EventKind,payload:Value,raw:Value)->EventEnvelope { native_harness_event_mode(run_id,trace_id,sequence,harness,IntegrationMode::StructuredStream,source,kind,payload,raw) }
pub fn native_harness_event_mode(run_id:Uuid,trace_id:Uuid,sequence:&mut u64,harness:HarnessId,mode:IntegrationMode,source:&str,kind:EventKind,payload:Value,raw:Value)->EventEnvelope { *sequence+=1; let mut event=EventEnvelope::new(run_id,trace_id,*sequence,harness,mode,Provenance::native(source),kind,payload); event.raw_source=Some(RawSource{source:source.into(),media_type:"application/json".into(),data:raw}); event }
pub fn captured_stdout_event(run_id:Uuid,trace_id:Uuid,sequence:&mut u64,harness:HarnessId,text:&str)->EventEnvelope { captured_stdout_event_mode(run_id,trace_id,sequence,harness,IntegrationMode::StructuredStream,text) }
pub fn captured_stdout_event_mode(run_id:Uuid,trace_id:Uuid,sequence:&mut u64,harness:HarnessId,mode:IntegrationMode,text:&str)->EventEnvelope { *sequence+=1; EventEnvelope::new(run_id,trace_id,*sequence,harness,mode,Provenance::native("agenttrace-process:stdout"),EventKind::ProcessStdout,json!({"text":text})) }
fn process_envelope(run_id:Uuid,trace_id:Uuid,sequence:&mut u64,harness:HarnessId,mode:IntegrationMode,kind:EventKind,payload:Value)->EventEnvelope { *sequence+=1; EventEnvelope::new(run_id,trace_id,*sequence,harness,mode,Provenance::native("agenttrace-process"),kind,payload) }
