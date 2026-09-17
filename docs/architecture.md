# Architecture

## Design goals

AgentTrace is local-first, evidence-first, crash-tolerant, cross-platform, and explicit about observability gaps. Core code should add little overhead to the traced process and must not require a cloud account.

The architecture separates **collection** from **normalization**. A collector records the strongest legitimate upstream signal available. A normalizer maps that signal into the AgentTrace protocol while preserving source evidence and provenance. Storage never assumes that two harnesses expose equivalent information.

## Workspace

```text
crates/
  agenttrace-protocol/       # versioned normalized event model
  agenttrace-adapter-api/    # adapter traits and capability reporting
  agenttrace-process/        # child process supervision
  agenttrace-collector/      # ingestion helpers and sequencing
  agenttrace-storage/        # SQLite, migrations, compression, run recovery
  agenttrace-redaction/      # secret/path/environment-value filtering primitives
  agenttrace-registry/       # built-in adapter registry
  agenttrace-replay/         # dry-run-first allowlisted shell replay
  agenttrace-cli/            # native agenttrace CLI
  agenttrace-server/         # loopback-first local HTTP API + binary
  adapters/*                 # harness-specific collectors/normalizers
apps/
  desktop/                   # independent Tauri v2 + React/TypeScript inspector
```

The desktop Tauri crate is intentionally a nested independent Cargo workspace so Tauri's runtime MSRV can move without raising the MSRV of the core Rust workspace.

## Data flow

1. **Detection** identifies an installed harness/version and the integration surfaces actually available on that machine.
2. **Capability negotiation** returns a report per signal (`native`, `inferred`, `derived`, or `unavailable`) with source notes.
3. **Collection** reads a structured stream, documented hook, transcript/session, logfile, extension/storage surface, or wrapped process output depending on the adapter.
4. **Normalization** creates `EventEnvelope` records and never upgrades provenance. A deterministic calculation from native data is `derived`, not `native`.
5. **Redaction** is applied by the CLI ingestion/import path before events are appended to normal durable storage. Stored raw-source fields therefore contain the redacted form seen by the persistence layer.
6. **Storage** appends events incrementally to SQLite in sequence order, compresses sufficiently large raw JSON blobs, maintains run summaries, and marks interrupted runs during recovery.
7. **Query surfaces** read from the same `TraceStore`: the CLI, the loopback HTTP API, and the Tauri command layer.
8. **UI** renders stored evidence and does not synthesize unavailable telemetry client-side.

Library consumers that call lower-level crates directly are responsible for applying the same redaction policy before persistence if they bypass the CLI path.

## Identity and ordering

Each event has a globally unique event ID, run ID, trace ID, sequence number, wall-clock timestamp, and optional span/monotonic timing data. Harness-native identifiers can be preserved in payloads, attributes, or raw-source data.

Sequence is authoritative for local ingestion order. Wall-clock time is useful for human display but is not trusted for strict ordering across processes.

## Raw event preservation

Raw records are useful because upstream schemas evolve. A raw record is not automatically safe to persist. Normal AgentTrace CLI ingestion redacts the complete serialized event before storage, including raw-source JSON. A trace database should still be treated as sensitive engineering data because arbitrary upstream payloads can contain information that no static redaction rule recognizes.

## Process supervision

Process wrapping records process start, stdout/stderr chunks, exit, cancellation, and duration from AgentTrace's own supervisor. It does **not** imply visibility into model requests, tool calls, or file reads. Structured child output can add those signals only when the harness explicitly exposes them.

`ProcessSpec` supports an explicit environment map and optional environment clearing. The default supervised harness process inherits the caller's environment so the real harness can run normally; AgentTrace does not record a snapshot of that environment by default. Replay uses a stricter scrubbed environment because replayed historical commands are a different trust boundary.

## Storage strategy

SQLite is the system of record. The current store uses WAL mode, `synchronous=NORMAL`, a bounded connection pool, explicit migrations, indexed run/sequence access, and zstd compression for sufficiently large raw-source payloads. Events are committed incrementally so a crash does not require the entire run to be reconstructed from memory.

On startup, interrupted runs can be classified as `interrupted` instead of silently appearing successful.

## Replay safety

Replay only considers recorded `shell.command` events with structured command metadata. A normal replay invocation is a dry run. Actual execution requires `--execute` plus an exact command or sequence allowlist.

Allowed commands execute in a temporary detached Git worktree at a caller-selected revision, with a scrubbed environment and per-command timeout. This isolates normal repository changes from the caller's active worktree. It is **not an operating-system sandbox**: an allowlisted command still has the current user's OS permissions and may deliberately access resources outside the worktree.

Model calls, tool calls, MCP operations, approvals, file events, and other trace records are not automatically re-executed.

## Local API boundary

`agenttrace-server` exposes health, harness/capability metadata, run summaries, event retrieval, JSONL export, and deterministic run statistics. It binds to `127.0.0.1:4319` by default and refuses non-loopback binding unless the caller explicitly enables it.

The HTTP API is currently read-oriented; recording remains an adapter/CLI responsibility.

## Desktop boundary

The desktop application uses Tauri commands backed directly by `TraceStore` for local run/event access. React/TypeScript owns presentation. The UI provides a run browser, dense event timeline, search and category filters, provenance indicators, payload/raw/execution inspectors, and observed metrics without decorative telemetry or fake precision.

A separate local HTTP API exists for other clients, but the desktop app does not require a localhost server process to function.
