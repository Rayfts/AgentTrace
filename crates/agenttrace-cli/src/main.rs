use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use agenttrace_adapter_api::{Capability, HarnessAdapter, ImportRequest, RunRequest};
use agenttrace_protocol::{EventEnvelope, EventKind, Provenance, ProvenanceLevel};
use agenttrace_redaction::{REDACTED, Redactor};
use agenttrace_registry::AdapterRegistry;
use agenttrace_replay::{ReplayOptions, ReplayPolicy, build_plan, execute_plan};
use agenttrace_storage::{RunSummary, TraceStore};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use serde_json::{Map, Value, json};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "agenttrace",
    version,
    about = "Local-first debugger and observability for AI coding agents"
)]
struct Cli {
    /// SQLite trace database. Defaults to .agenttrace/agenttrace.db in the current directory.
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the ten built-in harness adapters and local installation state.
    Harnesses,
    /// Show evidence-backed telemetry capabilities for one or all harnesses.
    Capabilities {
        /// Harness name such as codex, claude-code, gemini, or roo-code.
        harness: Option<String>,
    },
    /// Check local harness installations and adapter integration modes.
    Doctor,
    /// Run a coding-agent command while recording its normalized event stream.
    Run {
        #[arg(long)]
        harness: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    /// Import a harness trace/session file through its adapter.
    Import {
        #[arg(long)]
        harness: String,
        path: PathBuf,
    },
    /// Inspect a stored run and its normalized events.
    Inspect {
        run_id: Uuid,
        /// Include redacted raw-source payloads in event output.
        #[arg(long)]
        raw: bool,
    },
    /// Export a stored run as newline-delimited AgentTrace JSON.
    Export {
        run_id: Uuid,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compare two stored runs using deterministic trace statistics.
    Compare { left: Uuid, right: Uuid },
    /// Plan or execute allowlisted recorded shell commands in a detached Git worktree.
    Replay {
        run_id: Uuid,
        /// Repository whose detached worktree should receive the replay.
        #[arg(long)]
        repo: PathBuf,
        /// Git revision to check out for replay.
        #[arg(long, default_value = "HEAD")]
        revision: String,
        /// Actually execute allowlisted commands. Without this flag replay is a dry run.
        #[arg(long)]
        execute: bool,
        /// Exact recorded command display string to permit. Repeatable.
        #[arg(long = "allow")]
        allow: Vec<String>,
        /// Recorded shell.command sequence number to permit. Repeatable.
        #[arg(long = "allow-sequence")]
        allow_sequence: Vec<u64>,
        /// Per-command execution timeout.
        #[arg(long, default_value_t = 300)]
        timeout_seconds: u64,
        /// Continue executing later allowlisted commands after one fails.
        #[arg(long)]
        continue_on_error: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let registry = AdapterRegistry::default();

    match cli.command {
        Command::Harnesses => print_harnesses(&registry).await?,
        Command::Capabilities { harness } => print_capabilities(&registry, harness.as_deref()).await?,
        Command::Doctor => doctor(&registry).await?,
        Command::Run {
            harness,
            cwd,
            command,
        } => run(&registry, cli.db.as_deref(), &harness, cwd, command).await?,
        Command::Import { harness, path } => {
            import(&registry, cli.db.as_deref(), &harness, path).await?
        }
        Command::Inspect { run_id, raw } => inspect(cli.db.as_deref(), run_id, raw).await?,
        Command::Export { run_id, output } => {
            export(cli.db.as_deref(), run_id, output.as_deref()).await?
        }
        Command::Compare { left, right } => compare(cli.db.as_deref(), left, right).await?,
        Command::Replay {
            run_id,
            repo,
            revision,
            execute,
            allow,
            allow_sequence,
            timeout_seconds,
            continue_on_error,
        } => {
            replay(
                cli.db.as_deref(),
                run_id,
                repo,
                revision,
                execute,
                allow,
                allow_sequence,
                timeout_seconds,
                continue_on_error,
            )
            .await?
        }
    }

    Ok(())
}

async fn print_harnesses(registry: &AdapterRegistry) -> Result<(), Box<dyn Error>> {
    let mut rows = Vec::new();
    for (harness, adapter) in registry.iter() {
        let detection = adapter.detect().await?;
        rows.push(json!({
            "harness": AdapterRegistry::canonical_name(harness),
            "installed": detection.installed,
            "executable": detection.executable,
            "version": detection.version,
            "integration_modes": detection.integration_modes,
            "notes": detection.notes,
        }));
    }
    print_json(&Value::Array(rows))?;
    Ok(())
}

async fn print_capabilities(
    registry: &AdapterRegistry,
    harness: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let adapters = selected_adapters(registry, harness)?;
    let mut reports = Vec::new();
    for adapter in adapters {
        reports.push(capability_report_json(adapter.as_ref()).await?);
    }
    print_json(&Value::Array(reports))?;
    Ok(())
}

async fn doctor(registry: &AdapterRegistry) -> Result<(), Box<dyn Error>> {
    let mut installed = 0_u64;
    let mut harnesses = Vec::new();
    for (harness, adapter) in registry.iter() {
        let detection = adapter.detect().await?;
        installed += u64::from(detection.installed);
        harnesses.push(json!({
            "harness": AdapterRegistry::canonical_name(harness),
            "installed": detection.installed,
            "version": detection.version,
            "integration_modes": detection.integration_modes,
            "notes": detection.notes,
        }));
    }
    print_json(&json!({
        "ok": true,
        "registered_harnesses": harnesses.len(),
        "installed_harnesses": installed,
        "harnesses": harnesses,
        "security": {
            "redaction": "enabled-before-persistence",
            "environment_capture": "disabled-by-default"
        }
    }))?;
    Ok(())
}

async fn run(
    registry: &AdapterRegistry,
    db: Option<&Path>,
    harness: &str,
    cwd: PathBuf,
    command: Vec<OsString>,
) -> Result<(), Box<dyn Error>> {
    let adapter = adapter_for(registry, harness)?;
    let store = open_store(db).await?;
    let redactor = Redactor::default();
    let run_id = Uuid::new_v4();
    let handle = adapter
        .start(RunRequest {
            run_id,
            cwd,
            argv: command,
            integration_mode: None,
        })
        .await?;
    let mut stream = adapter.events(&handle).await?;
    let mut last_event: Option<EventEnvelope> = None;
    let mut semantic_terminal_seen = false;

    eprintln!("agenttrace run {run_id}");
    while let Some(event) = stream.next().await {
        let event = redact_event(event?, &redactor)?;
        semantic_terminal_seen |= matches!(event.kind, EventKind::RunCompleted | EventKind::RunFailed);
        store.append_event(&event).await?;
        println!("{}", serde_json::to_string(&event)?);
        last_event = Some(event);
    }

    if !semantic_terminal_seen {
        if let Some(terminal) = derive_terminal_from_process_exit(last_event.as_ref()) {
            store.append_event(&terminal).await?;
            println!("{}", serde_json::to_string(&terminal)?);
        }
    }
    Ok(())
}

async fn import(
    registry: &AdapterRegistry,
    db: Option<&Path>,
    harness: &str,
    path: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let adapter = adapter_for(registry, harness)?;
    let store = open_store(db).await?;
    let redactor = Redactor::default();
    let run_id = Uuid::new_v4();
    let mut stream = adapter.import(ImportRequest { run_id, path }).await?;
    let mut count = 0_u64;

    while let Some(event) = stream.next().await {
        let event = redact_event(event?, &redactor)?;
        store.append_event(&event).await?;
        count += 1;
    }

    print_json(&json!({"run_id": run_id, "imported_events": count}))?;
    Ok(())
}

async fn inspect(db: Option<&Path>, run_id: Uuid, raw: bool) -> Result<(), Box<dyn Error>> {
    let store = open_store(db).await?;
    let summary = store
        .run_summary(run_id)
        .await?
        .ok_or_else(|| input_error(format!("run {run_id} was not found")))?;
    let mut events = store.load_run_events(run_id).await?;
    if !raw {
        for event in &mut events {
            event.raw_source = None;
        }
    }
    print_json(&json!({
        "summary": run_summary_json(&summary),
        "events": events,
    }))?;
    Ok(())
}

async fn export(
    db: Option<&Path>,
    run_id: Uuid,
    output: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let store = open_store(db).await?;
    let events = store.load_run_events(run_id).await?;
    if events.is_empty() && store.run_summary(run_id).await?.is_none() {
        return Err(input_error(format!("run {run_id} was not found")).into());
    }

    let mut body = String::new();
    for event in events {
        body.push_str(&serde_json::to_string(&event)?);
        body.push('\n');
    }
    if let Some(path) = output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, body).await?;
    } else {
        print!("{body}");
    }
    Ok(())
}

async fn compare(
    db: Option<&Path>,
    left: Uuid,
    right: Uuid,
) -> Result<(), Box<dyn Error>> {
    let store = open_store(db).await?;
    let left_events = store.load_run_events(left).await?;
    let right_events = store.load_run_events(right).await?;
    if left_events.is_empty() && store.run_summary(left).await?.is_none() {
        return Err(input_error(format!("run {left} was not found")).into());
    }
    if right_events.is_empty() && store.run_summary(right).await?.is_none() {
        return Err(input_error(format!("run {right} was not found")).into());
    }

    print_json(&json!({
        "left": {"run_id": left, "stats": trace_stats(&left_events)},
        "right": {"run_id": right, "stats": trace_stats(&right_events)},
    }))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn replay(
    db: Option<&Path>,
    run_id: Uuid,
    repo: PathBuf,
    revision: String,
    execute: bool,
    allow: Vec<String>,
    allow_sequence: Vec<u64>,
    timeout_seconds: u64,
    continue_on_error: bool,
) -> Result<(), Box<dyn Error>> {
    let store = open_store(db).await?;
    let events = store.load_run_events(run_id).await?;
    if events.is_empty() && store.run_summary(run_id).await?.is_none() {
        return Err(input_error(format!("run {run_id} was not found")).into());
    }

    let plan = build_plan(&events);
    if !execute {
        print_json(&json!({
            "mode": "dry_run",
            "run_id": run_id,
            "plan": plan,
            "next": "repeat with --execute and one or more exact --allow / --allow-sequence entries"
        }))?;
        return Ok(());
    }

    if allow.is_empty() && allow_sequence.is_empty() {
        return Err(input_error(
            "replay execution requires at least one exact --allow or --allow-sequence entry".into(),
        )
        .into());
    }
    if timeout_seconds == 0 {
        return Err(input_error("--timeout-seconds must be greater than zero".into()).into());
    }

    let policy = allow
        .into_iter()
        .fold(ReplayPolicy::new(), |policy, command| {
            policy.allow_command(command)
        });
    let policy = allow_sequence
        .into_iter()
        .fold(policy, |policy, sequence| policy.allow_sequence(sequence));
    let mut options = ReplayOptions::new(repo);
    options.revision = revision;
    options.timeout_per_command = Duration::from_secs(timeout_seconds);
    options.continue_on_error = continue_on_error;

    let report = execute_plan(&plan, &policy, &options).await?;
    print_json(&serde_json::to_value(report)?)?;
    Ok(())
}

async fn capability_report_json(adapter: &dyn HarnessAdapter) -> Result<Value, Box<dyn Error>> {
    let report = adapter.capabilities().await?;
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

fn selected_adapters(
    registry: &AdapterRegistry,
    harness: Option<&str>,
) -> Result<Vec<std::sync::Arc<dyn HarnessAdapter>>, Box<dyn Error>> {
    if let Some(harness) = harness {
        return Ok(vec![adapter_for(registry, harness)?]);
    }
    Ok(registry.iter().map(|(_, adapter)| adapter).collect())
}

fn adapter_for(
    registry: &AdapterRegistry,
    harness: &str,
) -> Result<std::sync::Arc<dyn HarnessAdapter>, Box<dyn Error>> {
    let id = AdapterRegistry::parse(harness)
        .ok_or_else(|| input_error(format!("unknown harness `{harness}`")))?;
    registry
        .get(id)
        .ok_or_else(|| input_error(format!("harness `{harness}` is not registered")).into())
}

async fn open_store(db: Option<&Path>) -> Result<TraceStore, Box<dyn Error>> {
    let path = match db {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()?
            .join(".agenttrace")
            .join("agenttrace.db"),
    };
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(TraceStore::open(path).await?)
}

fn redact_event(event: EventEnvelope, redactor: &Redactor) -> Result<EventEnvelope, Box<dyn Error>> {
    let mut value = serde_json::to_value(event)?;
    redact_sensitive_paths(&mut value, redactor);
    redactor.redact_json(&mut value);
    Ok(serde_json::from_value(value)?)
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

fn derive_terminal_from_process_exit(last: Option<&EventEnvelope>) -> Option<EventEnvelope> {
    let process_exit = last.filter(|event| event.kind == EventKind::ProcessExited)?;
    let success = process_exit.payload.get("success")?.as_bool()?;
    let mut provenance = Provenance {
        level: ProvenanceLevel::Derived,
        source: "agenttrace-cli:process-exit".into(),
        native_fields: vec!["payload.success".into()],
        unavailable_fields: Vec::new(),
        notes: Some(
            "semantic terminal event derived only because the harness stream ended at a native process exit"
                .into(),
        ),
    };
    if process_exit.harness == agenttrace_protocol::HarnessId::Unknown {
        provenance
            .unavailable_fields
            .push("harness-specific completion".into());
    }
    Some(EventEnvelope::new(
        process_exit.run_id,
        process_exit.trace_id,
        process_exit.sequence.saturating_add(1),
        process_exit.harness,
        process_exit.integration_mode,
        provenance,
        if success {
            EventKind::RunCompleted
        } else {
            EventKind::RunFailed
        },
        json!({
            "derived_from": "process.exited",
            "success": success,
            "exit_code": process_exit.payload.get("code"),
        }),
    ))
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
        if let Some(duration) = event.duration_ns {
            duration_ns = duration_ns.saturating_add(duration);
        }
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

fn run_summary_json(summary: &RunSummary) -> Value {
    json!({
        "run_id": summary.run_id,
        "trace_id": summary.trace_id,
        "harness": summary.harness,
        "integration_mode": summary.integration_mode,
        "status": summary.status,
        "started_at": summary.started_at,
        "finished_at": summary.finished_at,
        "last_sequence": summary.last_sequence,
        "event_count": summary.event_count,
    })
}

fn print_json(value: &Value) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn input_error(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
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

#[cfg(test)]
mod tests {
    use agenttrace_protocol::{HarnessId, IntegrationMode};

    use super::*;

    #[test]
    fn derives_terminal_only_from_process_exit() {
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let event = EventEnvelope::new(
            run_id,
            trace_id,
            4,
            HarnessId::Aider,
            IntegrationMode::ProcessWrap,
            Provenance::native("test"),
            EventKind::ProcessExited,
            json!({"code": 0, "success": true}),
        );
        let terminal = derive_terminal_from_process_exit(Some(&event)).unwrap();
        assert_eq!(terminal.kind, EventKind::RunCompleted);
        assert_eq!(terminal.sequence, 5);
        assert_eq!(terminal.provenance.level, ProvenanceLevel::Derived);
    }

    #[test]
    fn trace_stats_keep_costs_partitioned_by_currency() {
        let event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("test"),
            EventKind::Checkpoint,
            json!({}),
        );
        let stats = trace_stats(&[event]);
        assert_eq!(stats["event_count"], 1);
        assert_eq!(stats["provenance"]["native"], 1);
    }
}
