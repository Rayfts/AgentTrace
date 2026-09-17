use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::{mpsc, oneshot},
};

const CHUNK_SIZE: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub clear_env: bool,
}

impl ProcessSpec {
    pub fn new(program: impl Into<OsString>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
            clear_env: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Started { pid: Option<u32> },
    Stdout { bytes: Vec<u8> },
    Stderr { bytes: Vec<u8> },
    Exited {
        code: Option<i32>,
        success: bool,
        duration: Duration,
    },
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("process I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("process event stream closed before startup")]
    EventStreamClosed,
}

pub struct SupervisedProcess {
    events: mpsc::Receiver<Result<ProcessEvent, ProcessError>>,
    cancel: Option<oneshot::Sender<()>>,
}

impl SupervisedProcess {
    pub async fn spawn(spec: ProcessSpec) -> Result<Self, ProcessError> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if spec.clear_env {
            command.env_clear();
        }
        command.envs(spec.env);

        let mut child = command.spawn()?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let started = Instant::now();
        let (tx, rx) = mpsc::channel(256);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        tx.send(Ok(ProcessEvent::Started { pid }))
            .await
            .map_err(|_| ProcessError::EventStreamClosed)?;

        if let Some(stdout) = stdout {
            tokio::spawn(read_stream(stdout, tx.clone(), StreamKind::Stdout));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(read_stream(stderr, tx.clone(), StreamKind::Stderr));
        }

        tokio::spawn(async move {
            let status = tokio::select! {
                result = child.wait() => result,
                _ = &mut cancel_rx => {
                    if let Err(error) = child.kill().await {
                        let _ = tx.send(Err(ProcessError::Io(error))).await;
                    }
                    child.wait().await
                }
            };
            match status {
                Ok(status) => {
                    let _ = tx
                        .send(Ok(ProcessEvent::Exited {
                            code: status.code(),
                            success: status.success(),
                            duration: started.elapsed(),
                        }))
                        .await;
                }
                Err(error) => {
                    let _ = tx.send(Err(ProcessError::Io(error))).await;
                }
            }
        });

        Ok(Self {
            events: rx,
            cancel: Some(cancel_tx),
        })
    }

    pub async fn next_event(&mut self) -> Option<Result<ProcessEvent, ProcessError>> {
        self.events.recv().await
    }

    pub fn cancel(&mut self) -> bool {
        self.cancel.take().is_some_and(|sender| sender.send(()).is_ok())
    }
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

async fn read_stream<R>(
    mut reader: R,
    sender: mpsc::Sender<Result<ProcessEvent, ProcessError>>,
    kind: StreamKind,
) where
    R: AsyncRead + Unpin,
{
    let mut buffer = vec![0_u8; CHUNK_SIZE];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let bytes = buffer[..read].to_vec();
                let event = match kind {
                    StreamKind::Stdout => ProcessEvent::Stdout { bytes },
                    StreamKind::Stderr => ProcessEvent::Stderr { bytes },
                };
                if sender.send(Ok(event)).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(ProcessError::Io(error))).await;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_spec_does_not_clear_or_record_environment_by_default() {
        let spec = ProcessSpec::new("agent", ".");
        assert!(!spec.clear_env);
        assert!(spec.env.is_empty());
    }
}
