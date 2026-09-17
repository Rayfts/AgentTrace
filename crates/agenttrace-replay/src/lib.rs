use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use agenttrace_protocol::{EventEnvelope, EventKind};
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::{process::Command, time::timeout};
use uuid::Uuid;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReplayCommand {
    pub event_id: Uuid,
    pub sequence: u64,
    pub program: String,
    pub args: Vec<String>,
    pub display: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayPlan {
    pub source_run_id: Option<Uuid>,
    pub commands: Vec<ReplayCommand>,
    pub non_shell_events: usize,
    pub safety: ReplaySafety,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplaySafety {
    pub dry_run_by_default: bool,
    pub exact_allowlist_required: bool,
    pub environment_scrubbed: bool,
    pub detached_git_worktree: bool,
    pub os_sandboxed: bool,
    pub notes: &'static str,
}

impl Default for ReplaySafety {
    fn default() -> Self {
        Self {
            dry_run_by_default: true,
            exact_allowlist_required: true,
            environment_scrubbed: true,
            detached_git_worktree: true,
            os_sandboxed: false,
            notes: "Replay isolates repository state in a detached Git worktree, but it is not an operating-system sandbox. Allowlisted commands still execute with the current user's OS permissions.",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplayPolicy {
    allowed_commands: BTreeSet<String>,
    allowed_sequences: BTreeSet<u64>,
}

impl ReplayPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_command(mut self, command: impl Into<String>) -> Self {
        self.allowed_commands.insert(command.into());
        self
    }

    pub fn allow_sequence(mut self, sequence: u64) -> Self {
        self.allowed_sequences.insert(sequence);
        self
    }

    pub fn allows(&self, command: &ReplayCommand) -> bool {
        self.allowed_sequences.contains(&command.sequence)
            || self.allowed_commands.contains(&command.display)
    }

    pub fn is_empty(&self) -> bool {
        self.allowed_commands.is_empty() && self.allowed_sequences.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ReplayOptions {
    pub repo: PathBuf,
    pub revision: String,
    pub timeout_per_command: Duration,
    pub continue_on_error: bool,
}

impl ReplayOptions {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            revision: "HEAD".into(),
            timeout_per_command: DEFAULT_TIMEOUT,
            continue_on_error: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayOutcome {
    Blocked,
    Succeeded,
    Failed,
    TimedOut,
    SkippedAfterFailure,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayResult {
    pub command: ReplayCommand,
    pub outcome: ReplayOutcome,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    pub source_run_id: Option<Uuid>,
    pub repository: String,
    pub revision: String,
    pub results: Vec<ReplayResult>,
    pub executed: usize,
    pub blocked: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub cleanup_warning: Option<String>,
    pub safety: ReplaySafety,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("Git repository validation failed: {0}")]
    InvalidRepository(String),
    #[error("failed to create replay workspace: {0}")]
    Workspace(String),
    #[error("replay I/O error: {0}")]
    Io(#[from] std::io::Error),
}

struct ReplayWorkspace {
    _temp: TempDir,
    path: PathBuf,
}

impl ReplayWorkspace {
    fn path(&self) -> &Path {
        &self.path
    }
}

pub fn build_plan(events: &[EventEnvelope]) -> ReplayPlan {
    let source_run_id = events.first().map(|event| event.run_id);
    let mut commands = Vec::new();
    let mut non_shell_events = 0_usize;

    for event in events {
        if event.kind != EventKind::ShellCommand {
            non_shell_events += 1;
            continue;
        }
        let Some(command) = &event.command else {
            non_shell_events += 1;
            continue;
        };
        commands.push(ReplayCommand {
            event_id: event.event_id,
            sequence: event.sequence,
            program: command.program.clone(),
            args: command.args.clone(),
            display: command_display(&command.program, &command.args),
        });
    }

    ReplayPlan {
        source_run_id,
        commands,
        non_shell_events,
        safety: ReplaySafety::default(),
    }
}

pub async fn execute_plan(
    plan: &ReplayPlan,
    policy: &ReplayPolicy,
    options: &ReplayOptions,
) -> Result<ReplayReport, ReplayError> {
    let repository = repository_root(&options.repo).await?;
    let allowed_count = plan
        .commands
        .iter()
        .filter(|command| policy.allows(command))
        .count();

    if allowed_count == 0 {
        let results = plan
            .commands
            .iter()
            .cloned()
            .map(blocked_result)
            .collect::<Vec<_>>();
        return Ok(summarize_report(
            plan,
            &repository,
            options,
            results,
            None,
        ));
    }

    let workspace = create_worktree(&repository, &options.revision).await?;
    let workspace_path = workspace.path().to_path_buf();
    let mut results = Vec::with_capacity(plan.commands.len());
    let mut failed = false;

    for command in &plan.commands {
        if !policy.allows(command) {
            results.push(blocked_result(command.clone()));
            continue;
        }
        if failed && !options.continue_on_error {
            results.push(ReplayResult {
                command: command.clone(),
                outcome: ReplayOutcome::SkippedAfterFailure,
                exit_code: None,
                duration_ms: None,
                stdout: String::new(),
                stderr: "not executed because an earlier allowlisted replay command failed".into(),
            });
            continue;
        }

        let result = execute_command(command, &workspace_path, options.timeout_per_command).await;
        failed |= matches!(
            result.outcome,
            ReplayOutcome::Failed | ReplayOutcome::TimedOut
        );
        results.push(result);
    }

    let cleanup_warning = remove_worktree(&repository, &workspace_path)
        .await
        .err()
        .map(|error| error.to_string());
    drop(workspace);

    Ok(summarize_report(
        plan,
        &repository,
        options,
        results,
        cleanup_warning,
    ))
}

async fn repository_root(repo: &Path) -> Result<PathBuf, ReplayError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| ReplayError::InvalidRepository(error.to_string()))?;
    if !output.status.success() {
        return Err(ReplayError::InvalidRepository(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if root.is_empty() {
        return Err(ReplayError::InvalidRepository(
            "git rev-parse returned an empty repository path".into(),
        ));
    }
    Ok(PathBuf::from(root))
}

async fn create_worktree(repo: &Path, revision: &str) -> Result<ReplayWorkspace, ReplayError> {
    let temp = tempfile::Builder::new()
        .prefix("agenttrace-replay-")
        .tempdir()
        .map_err(|error| ReplayError::Workspace(error.to_string()))?;
    let path = temp.path().join("worktree");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add", "--detach"])
        .arg(&path)
        .arg(revision)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| ReplayError::Workspace(error.to_string()))?;
    if !output.status.success() {
        return Err(ReplayError::Workspace(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(ReplayWorkspace { _temp: temp, path })
}

async fn remove_worktree(repo: &Path, workspace: &Path) -> Result<(), ReplayError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(workspace)
        .stdin(Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        return Err(ReplayError::Workspace(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(())
}

async fn execute_command(command: &ReplayCommand, cwd: &Path, limit: Duration) -> ReplayResult {
    let mut process = replay_process(command);
    process
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    copy_safe_environment(&mut process);

    let started = Instant::now();
    let child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ReplayResult {
                command: command.clone(),
                outcome: ReplayOutcome::Failed,
                exit_code: None,
                duration_ms: Some(duration_ms(started.elapsed())),
                stdout: String::new(),
                stderr: format!("failed to spawn replay command: {error}"),
            };
        }
    };

    match timeout(limit, child.wait_with_output()).await {
        Ok(Ok(output)) => ReplayResult {
            command: command.clone(),
            outcome: if output.status.success() {
                ReplayOutcome::Succeeded
            } else {
                ReplayOutcome::Failed
            },
            exit_code: output.status.code(),
            duration_ms: Some(duration_ms(started.elapsed())),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Ok(Err(error)) => ReplayResult {
            command: command.clone(),
            outcome: ReplayOutcome::Failed,
            exit_code: None,
            duration_ms: Some(duration_ms(started.elapsed())),
            stdout: String::new(),
            stderr: format!("failed while waiting for replay command: {error}"),
        },
        Err(_) => ReplayResult {
            command: command.clone(),
            outcome: ReplayOutcome::TimedOut,
            exit_code: None,
            duration_ms: Some(duration_ms(started.elapsed())),
            stdout: String::new(),
            stderr: format!("command exceeded replay timeout of {}s", limit.as_secs()),
        },
    }
}

fn replay_process(command: &ReplayCommand) -> Command {
    if command.program == "shell" {
        let text = command.args.join(" ");
        #[cfg(windows)]
        {
            let mut process = Command::new("cmd.exe");
            process.args(["/D", "/S", "/C"]).arg(text);
            process
        }
        #[cfg(not(windows))]
        {
            let mut process = Command::new("sh");
            process.args(["-lc"]).arg(text);
            process
        }
    } else {
        let mut process = Command::new(&command.program);
        process.args(&command.args);
        process
    }
}

fn copy_safe_environment(command: &mut Command) {
    for name in [
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn command_display(program: &str, args: &[String]) -> String {
    if program == "shell" {
        return args.join(" ");
    }
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(display_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_arg(value: &str) -> String {
    if value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace() || "\"'`$&;|<>*?()[]{}".contains(character)
        })
    {
        serde_json::to_string(value).unwrap_or_else(|_| "\"<unprintable>\"".into())
    } else {
        value.to_owned()
    }
}

fn blocked_result(command: ReplayCommand) -> ReplayResult {
    ReplayResult {
        command,
        outcome: ReplayOutcome::Blocked,
        exit_code: None,
        duration_ms: None,
        stdout: String::new(),
        stderr: "command is not present in the replay allowlist".into(),
    }
}

fn summarize_report(
    plan: &ReplayPlan,
    repository: &Path,
    options: &ReplayOptions,
    results: Vec<ReplayResult>,
    cleanup_warning: Option<String>,
) -> ReplayReport {
    let executed = results
        .iter()
        .filter(|result| {
            matches!(
                result.outcome,
                ReplayOutcome::Succeeded | ReplayOutcome::Failed | ReplayOutcome::TimedOut
            )
        })
        .count();
    let blocked = results
        .iter()
        .filter(|result| result.outcome == ReplayOutcome::Blocked)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.outcome == ReplayOutcome::Failed)
        .count();
    let timed_out = results
        .iter()
        .filter(|result| result.outcome == ReplayOutcome::TimedOut)
        .count();

    ReplayReport {
        source_run_id: plan.source_run_id,
        repository: repository.to_string_lossy().into_owned(),
        revision: options.revision.clone(),
        results,
        executed,
        blocked,
        failed,
        timed_out,
        cleanup_warning,
        safety: ReplaySafety::default(),
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use agenttrace_protocol::{CommandInfo, HarnessId, IntegrationMode, Provenance};
    use serde_json::json;

    use super::*;

    fn shell_event(sequence: u64, command: &str) -> EventEnvelope {
        let mut event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            sequence,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::ShellCommand,
            json!({"command": command}),
        );
        event.command = Some(CommandInfo {
            program: "shell".into(),
            args: vec![command.into()],
            cwd: None,
            exit_code: None,
        });
        event
    }

    #[test]
    fn plan_only_contains_shell_command_events() {
        let shell = shell_event(3, "cargo test");
        let other = EventEnvelope::new(
            shell.run_id,
            shell.trace_id,
            4,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::ModelResponse,
            json!({}),
        );
        let plan = build_plan(&[shell, other]);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].display, "cargo test");
        assert_eq!(plan.non_shell_events, 1);
        assert!(!plan.safety.os_sandboxed);
    }

    #[test]
    fn policy_requires_exact_command_or_sequence() {
        let command = ReplayCommand {
            event_id: Uuid::new_v4(),
            sequence: 9,
            program: "shell".into(),
            args: vec!["cargo test".into()],
            display: "cargo test".into(),
        };
        assert!(!ReplayPolicy::new().allows(&command));
        assert!(
            ReplayPolicy::new()
                .allow_command("cargo test")
                .allows(&command)
        );
        assert!(ReplayPolicy::new().allow_sequence(9).allows(&command));
        assert!(
            !ReplayPolicy::new()
                .allow_command("cargo test --all")
                .allows(&command)
        );
    }

    #[test]
    fn structured_display_quotes_ambiguous_arguments() {
        assert_eq!(
            command_display("cargo", &["test".into(), "my test".into()]),
            "cargo test \"my test\""
        );
    }
}
