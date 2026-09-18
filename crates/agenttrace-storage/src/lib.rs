use std::{path::Path, str::FromStr, time::Duration};

use agenttrace_protocol::{EventEnvelope, EventKind};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use thiserror::Error;
use uuid::Uuid;

const COMPRESSION_THRESHOLD_BYTES: usize = 4 * 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] MigrateError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("compression error: {0}")]
    Compression(#[from] std::io::Error),
    #[error("invalid UUID in database: {0}")]
    InvalidUuid(#[from] uuid::Error),
    #[error("event sequence {sequence} does not fit SQLite INTEGER")]
    SequenceOverflow { sequence: u64 },
    #[error("artifact size {size} does not fit SQLite INTEGER")]
    ArtifactSizeOverflow { size: usize },
    #[error("artifact event {event_id} does not exist")]
    ArtifactEventNotFound { event_id: Uuid },
    #[error("artifact event {event_id} belongs to run {event_run_id}, not requested run {run_id}")]
    ArtifactEventRunMismatch {
        event_id: Uuid,
        event_run_id: Uuid,
        run_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub trace_id: Uuid,
    pub harness: String,
    pub integration_mode: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub last_sequence: u64,
    pub event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMetadata {
    pub artifact_id: Uuid,
    pub run_id: Uuid,
    pub event_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub media_type: Option<String>,
    pub content_sha256: String,
    pub original_size: u64,
    pub compressed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    pub metadata: ArtifactMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct TraceStore {
    pool: SqlitePool,
}

impl TraceStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let url = format!("sqlite://{}", path.as_ref().to_string_lossy());
        let options = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        Self::migrate(pool).await
    }

    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::migrate(pool).await
    }

    async fn migrate(pool: SqlitePool) -> Result<Self, StorageError> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn recover_interrupted_runs(&self) -> Result<u64, StorageError> {
        let result = sqlx::query(
            "UPDATE runs SET status = 'interrupted', finished_at = COALESCE(finished_at, ?) WHERE status = 'running'",
        )
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn append_event(&self, event: &EventEnvelope) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        Self::append_event_tx(&mut tx, event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn append_batch(&self, events: &[EventEnvelope]) -> Result<(), StorageError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for event in events {
            Self::append_event_tx(&mut tx, event).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn append_event_tx(
        tx: &mut Transaction<'_, Sqlite>,
        event: &EventEnvelope,
    ) -> Result<(), StorageError> {
        let sequence =
            i64::try_from(event.sequence).map_err(|_| StorageError::SequenceOverflow {
                sequence: event.sequence,
            })?;
        let harness = serde_json::to_value(event.harness)?
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        let integration_mode = serde_json::to_value(event.integration_mode)?
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        let kind = serde_json::to_value(event.kind)?
            .as_str()
            .unwrap_or("error")
            .to_owned();
        let provenance = serde_json::to_value(event.provenance.level)?
            .as_str()
            .unwrap_or("unavailable")
            .to_owned();

        sqlx::query(
            "INSERT INTO runs (run_id, trace_id, harness, integration_mode, status, started_at, last_sequence, event_count) \
             VALUES (?, ?, ?, ?, 'running', ?, 0, 0) \
             ON CONFLICT(run_id) DO NOTHING",
        )
        .bind(event.run_id.to_string())
        .bind(event.trace_id.to_string())
        .bind(&harness)
        .bind(&integration_mode)
        .bind(event.timestamp.to_rfc3339())
        .execute(&mut **tx)
        .await?;

        let serialized = serde_json::to_vec(event)?;
        let (blob, compressed) = encode_payload(serialized)?;

        sqlx::query(
            "INSERT INTO events (event_id, run_id, trace_id, sequence, kind, timestamp, harness, integration_mode, provenance, event_blob, compressed, raw_present) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event.event_id.to_string())
        .bind(event.run_id.to_string())
        .bind(event.trace_id.to_string())
        .bind(sequence)
        .bind(kind)
        .bind(event.timestamp.to_rfc3339())
        .bind(harness)
        .bind(integration_mode)
        .bind(provenance)
        .bind(blob)
        .bind(i64::from(compressed))
        .bind(i64::from(event.raw_source.is_some()))
        .execute(&mut **tx)
        .await?;

        let status = match event.kind {
            EventKind::RunCompleted => Some("completed"),
            EventKind::RunFailed => Some("failed"),
            _ => None,
        };
        if let Some(status) = status {
            sqlx::query(
                "UPDATE runs SET status = ?, finished_at = ?, last_sequence = MAX(last_sequence, ?), event_count = event_count + 1 WHERE run_id = ?",
            )
            .bind(status)
            .bind(event.timestamp.to_rfc3339())
            .bind(sequence)
            .bind(event.run_id.to_string())
            .execute(&mut **tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE runs SET last_sequence = MAX(last_sequence, ?), event_count = event_count + 1 WHERE run_id = ?",
            )
            .bind(sequence)
            .bind(event.run_id.to_string())
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    pub async fn load_run_events(&self, run_id: Uuid) -> Result<Vec<EventEnvelope>, StorageError> {
        let rows = sqlx::query(
            "SELECT event_blob, compressed FROM events WHERE run_id = ? ORDER BY sequence ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let blob: Vec<u8> = row.try_get("event_blob")?;
                let compressed: i64 = row.try_get("compressed")?;
                let bytes = decode_payload(blob, compressed != 0)?;
                Ok(serde_json::from_slice(&bytes)?)
            })
            .collect()
    }

    pub async fn run_summary(&self, run_id: Uuid) -> Result<Option<RunSummary>, StorageError> {
        let row = sqlx::query(
            "SELECT run_id, trace_id, harness, integration_mode, status, started_at, finished_at, last_sequence, event_count FROM runs WHERE run_id = ?",
        )
        .bind(run_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_summary).transpose()
    }

    pub async fn list_runs(&self, limit: u32) -> Result<Vec<RunSummary>, StorageError> {
        let rows = sqlx::query(
            "SELECT run_id, trace_id, harness, integration_mode, status, started_at, finished_at, last_sequence, event_count FROM runs ORDER BY started_at DESC LIMIT ?",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_summary).collect()
    }

    pub async fn store_artifact(
        &self,
        run_id: Uuid,
        event_id: Option<Uuid>,
        name: impl Into<String>,
        kind: impl Into<String>,
        media_type: Option<String>,
        bytes: &[u8],
    ) -> Result<ArtifactMetadata, StorageError> {
        let original_size =
            i64::try_from(bytes.len()).map_err(|_| StorageError::ArtifactSizeOverflow {
                size: bytes.len(),
            })?;
        let name = name.into();
        let kind = kind.into();
        let artifact_id = Uuid::new_v4();
        let created_at = Utc::now();
        let content_sha256 = format!("{:x}", Sha256::digest(bytes));
        let (blob, compressed) = encode_payload(bytes.to_vec())?;
        let mut tx = self.pool.begin().await?;

        if let Some(event_id) = event_id {
            let event_run_id: Option<String> =
                sqlx::query_scalar("SELECT run_id FROM events WHERE event_id = ?")
                    .bind(event_id.to_string())
                    .fetch_optional(&mut *tx)
                    .await?;
            let Some(event_run_id) = event_run_id else {
                return Err(StorageError::ArtifactEventNotFound { event_id });
            };
            let event_run_id = Uuid::parse_str(&event_run_id)?;
            if event_run_id != run_id {
                return Err(StorageError::ArtifactEventRunMismatch {
                    event_id,
                    event_run_id,
                    run_id,
                });
            }
        }

        sqlx::query(
            "INSERT INTO artifacts (artifact_id, run_id, event_id, name, kind, media_type, content_sha256, original_size, artifact_blob, compressed, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(artifact_id.to_string())
        .bind(run_id.to_string())
        .bind(event_id.map(|value| value.to_string()))
        .bind(&name)
        .bind(&kind)
        .bind(&media_type)
        .bind(&content_sha256)
        .bind(original_size)
        .bind(blob)
        .bind(i64::from(compressed))
        .bind(created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(ArtifactMetadata {
            artifact_id,
            run_id,
            event_id,
            name,
            kind,
            media_type,
            content_sha256,
            original_size: original_size as u64,
            compressed,
            created_at,
        })
    }

    pub async fn list_artifacts(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ArtifactMetadata>, StorageError> {
        let rows = sqlx::query(
            "SELECT artifact_id, run_id, event_id, name, kind, media_type, content_sha256, original_size, compressed, created_at \
             FROM artifacts WHERE run_id = ? ORDER BY created_at ASC, artifact_id ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_artifact_metadata).collect()
    }

    pub async fn load_artifact(
        &self,
        artifact_id: Uuid,
    ) -> Result<Option<StoredArtifact>, StorageError> {
        let row = sqlx::query(
            "SELECT artifact_id, run_id, event_id, name, kind, media_type, content_sha256, original_size, compressed, created_at, artifact_blob \
             FROM artifacts WHERE artifact_id = ?",
        )
        .bind(artifact_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| {
            let blob: Vec<u8> = row.try_get("artifact_blob")?;
            let compressed: i64 = row.try_get("compressed")?;
            let bytes = decode_payload(blob, compressed != 0)?;
            let metadata = row_to_artifact_metadata(row)?;
            Ok(StoredArtifact { metadata, bytes })
        })
        .transpose()
    }
}

fn row_to_summary(row: sqlx::sqlite::SqliteRow) -> Result<RunSummary, StorageError> {
    let last_sequence: i64 = row.try_get("last_sequence")?;
    let event_count: i64 = row.try_get("event_count")?;
    let started_at: String = row.try_get("started_at")?;
    let finished_at: Option<String> = row.try_get("finished_at")?;
    Ok(RunSummary {
        run_id: Uuid::parse_str(row.try_get::<String, _>("run_id")?.as_str())?,
        trace_id: Uuid::parse_str(row.try_get::<String, _>("trace_id")?.as_str())?,
        harness: row.try_get("harness")?,
        integration_mode: row.try_get("integration_mode")?,
        status: row.try_get("status")?,
        started_at: DateTime::parse_from_rfc3339(&started_at)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?
            .with_timezone(&Utc),
        finished_at: finished_at
            .map(|value| {
                DateTime::parse_from_rfc3339(&value)
                    .map(|value| value.with_timezone(&Utc))
                    .map_err(|error| sqlx::Error::Decode(Box::new(error)))
            })
            .transpose()?,
        last_sequence: u64::try_from(last_sequence).unwrap_or_default(),
        event_count: u64::try_from(event_count).unwrap_or_default(),
    })
}

fn row_to_artifact_metadata(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ArtifactMetadata, StorageError> {
    let event_id: Option<String> = row.try_get("event_id")?;
    let original_size: i64 = row.try_get("original_size")?;
    let compressed: i64 = row.try_get("compressed")?;
    let created_at: String = row.try_get("created_at")?;
    Ok(ArtifactMetadata {
        artifact_id: Uuid::parse_str(row.try_get::<String, _>("artifact_id")?.as_str())?,
        run_id: Uuid::parse_str(row.try_get::<String, _>("run_id")?.as_str())?,
        event_id: event_id
            .map(|value| Uuid::parse_str(&value))
            .transpose()?,
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        media_type: row.try_get("media_type")?,
        content_sha256: row.try_get("content_sha256")?,
        original_size: u64::try_from(original_size).unwrap_or_default(),
        compressed: compressed != 0,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?
            .with_timezone(&Utc),
    })
}

fn encode_payload(bytes: Vec<u8>) -> Result<(Vec<u8>, bool), StorageError> {
    if bytes.len() < COMPRESSION_THRESHOLD_BYTES {
        return Ok((bytes, false));
    }
    Ok((zstd::stream::encode_all(bytes.as_slice(), 1)?, true))
}

fn decode_payload(bytes: Vec<u8>, compressed: bool) -> Result<Vec<u8>, StorageError> {
    if !compressed {
        return Ok(bytes);
    }
    Ok(zstd::stream::decode_all(bytes.as_slice())?)
}

#[cfg(test)]
mod tests {
    use agenttrace_protocol::{HarnessId, IntegrationMode, Provenance};
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn round_trips_events_in_sequence() {
        let store = TraceStore::open_in_memory().await.unwrap();
        let run_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let first = EventEnvelope::new(
            run_id,
            trace_id,
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunStarted,
            json!({"ok": true}),
        );
        let second = EventEnvelope::new(
            run_id,
            trace_id,
            2,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunCompleted,
            json!({"answer": "done"}),
        );
        store
            .append_batch(&[first.clone(), second.clone()])
            .await
            .unwrap();
        assert_eq!(
            store.load_run_events(run_id).await.unwrap(),
            vec![first, second]
        );
        let summary = store.run_summary(run_id).await.unwrap().unwrap();
        assert_eq!(summary.status, "completed");
        assert_eq!(summary.event_count, 2);
    }

    #[tokio::test]
    async fn marks_unfinished_runs_interrupted() {
        let store = TraceStore::open_in_memory().await.unwrap();
        let event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Pi,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunStarted,
            json!({}),
        );
        store.append_event(&event).await.unwrap();
        assert_eq!(store.recover_interrupted_runs().await.unwrap(), 1);
        assert_eq!(
            store
                .run_summary(event.run_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "interrupted"
        );
    }

    #[tokio::test]
    async fn round_trips_compressed_run_artifacts() {
        let store = TraceStore::open_in_memory().await.unwrap();
        let event = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunStarted,
            json!({}),
        );
        store.append_event(&event).await.unwrap();

        let bytes = vec![b'x'; COMPRESSION_THRESHOLD_BYTES * 2];
        let metadata = store
            .store_artifact(
                event.run_id,
                Some(event.event_id),
                "terminal.log",
                "log",
                Some("text/plain".into()),
                &bytes,
            )
            .await
            .unwrap();
        assert!(metadata.compressed);
        assert_eq!(metadata.original_size, bytes.len() as u64);
        assert_eq!(metadata.content_sha256.len(), 64);
        assert_eq!(
            store.list_artifacts(event.run_id).await.unwrap(),
            vec![metadata.clone()]
        );

        let loaded = store
            .load_artifact(metadata.artifact_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.metadata, metadata);
        assert_eq!(loaded.bytes, bytes);
    }

    #[tokio::test]
    async fn rejects_artifact_event_links_across_runs() {
        let store = TraceStore::open_in_memory().await.unwrap();
        let first = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::Codex,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunStarted,
            json!({}),
        );
        let second = EventEnvelope::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            HarnessId::ClaudeCode,
            IntegrationMode::StructuredStream,
            Provenance::native("fixture"),
            EventKind::RunStarted,
            json!({}),
        );
        store.append_event(&first).await.unwrap();
        store.append_event(&second).await.unwrap();

        let error = store
            .store_artifact(
                first.run_id,
                Some(second.event_id),
                "mismatch.txt",
                "fixture",
                Some("text/plain".into()),
                b"data",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            StorageError::ArtifactEventRunMismatch { .. }
        ));
    }
}
