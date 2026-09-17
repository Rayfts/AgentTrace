# Architecture

## Design goals

AgentTrace is local-first, evidence-first, crash-tolerant, cross-platform, and explicit about observability gaps. Core code must add little overhead to the traced process and must not require a cloud account.

The architecture separates **collection** from **normalization**. A collector records the strongest legitimate upstream signal available. A normalizer maps that signal into the AgentTrace protocol while preserving the raw event and field-level provenance. Storage never assumes that two harnesses expose equivalent information.

## Planned workspace

```text
crates/
  agenttrace-protocol/       # versioned normalized event model
  agenttrace-adapter-api/    # adapter traits, capability reporting
  agenttrace-core/           # orchestration and normalization
  agenttrace-collector/      # ingestion pipeline and sequencing
  agenttrace-storage/        # SQLite + migrations + payload compression
  agenttrace-redaction/      # secret/path/environment filtering
  agenttrace-process/        # child process supervision
  agenttrace-mcp/            # MCP proxy/interceptor
  agenttrace-replay/         # policy-gated replay
  agenttrace-cli/            # native CLI
  adapters/*                 # harness-specific collectors/normalizers
apps/
  desktop/                   # Tauri shell + React/TypeScript view layer
```

## Data flow

1. **Detection** identifies an installed harness/version and the integration surfaces actually available on that machine.
2. **Capability negotiation** returns a report per signal (native/inferred/derived/unavailable), with evidence notes.
3. **Collection** reads a structured stream, documented hook, transcript/session, logfile, extension API, or wrapped process output.
4. **Normalization** creates `EventEnvelope` records and never upgrades provenance. A parser may derive duration from two native timestamps, but it cannot label the result native.
5. **Redaction** executes before durable storage for configured sensitive fields and again before export. Raw payload retention is policy-controlled because raw harness events can contain secrets.
6. **Storage** appends events incrementally to SQLite in sequence order. Large raw payloads and attachments may be compressed or stored out-of-row with content hashes.
7. **Query/API** provides run summaries, timeline/span queries, search, diffs, comparisons, and live subscriptions.
8. **UI** renders the same API. It does not invent telemetry client-side.

## Identity and ordering

Each event has a globally unique event ID, run ID, trace ID, sequence number, wall-clock timestamp, and optional monotonic timestamp. Harness-native span identifiers are preserved as attributes/raw data; AgentTrace span IDs are used to build a stable cross-harness hierarchy.

Sequence is authoritative for local ingestion order. Wall-clock time is useful for human display but is not trusted for strict ordering across processes. Where a harness provides timestamps, AgentTrace stores them in the normalized timestamp or attributes and records provenance.

## Raw event preservation

Raw records are important because upstream schemas evolve. A raw record is not automatically safe to persist: it first passes redaction policy. Collectors should preserve the original JSON object where possible rather than stringify/reparse arbitrary text.

## Process supervision

Process wrapping records start, stdout/stderr chunks, exit, cancellation, and duration natively from AgentTrace's own supervisor. It does **not** imply visibility into model requests, tool calls, or file reads. Structured child output can add those signals only when the harness explicitly exposes them.

Environment capture is allowlist-based. AgentTrace must never snapshot the complete environment by default.

## Storage strategy

SQLite is the system of record. The target configuration is WAL mode, `synchronous=NORMAL` for normal ingestion, short batched transactions, indexed `(run_id, sequence)` and `(run_id, kind, sequence)` access paths, and explicit schema migrations. Attachments use content hashes and sizes; large JSON/raw blobs can be zstd-compressed after a threshold.

Crash recovery should leave every committed event readable and classify an interrupted run on next open instead of silently marking it successful.

## Replay safety

Replay is opt-in and capability-specific. Read-only/pure operations can be replayed directly when their inputs are complete. Shell writes, file writes, Git mutations, network calls, external service calls, and credentialed operations require an explicit confirmation decision before execution. Imported traces never receive implicit trust.

## Desktop boundary

The desktop application is a client of the same Rust API used by `agenttrace serve`. Tauri owns native integration; React/TypeScript owns presentation. The UI should resemble a focused developer tool: dense timelines, trees, inspectors, terminal output, raw JSON, diffs, search, filters, bookmarks, and capability indicators without decorative telemetry or fake precision.
