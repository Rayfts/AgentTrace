# Architecture

## Design goals

AgentTrace is local-first, evidence-first, crash-tolerant, cross-platform, and explicit about observability gaps. Core code should add little overhead to the traced process and must not require a cloud account.

The architecture separates **collection** from **normalization**. A collector records the strongest legitimate upstream signal available. A normalizer maps that signal into the AgentTrace protocol while preserving source evidence and provenance. Storage never assumes that two harnesses expose equivalent information.

## Workspace

```text
crates/
  agenttrace-protocol/          # versioned normalized event model
  agenttrace-adapter-api/       # adapter traits and complete capability model
  agenttrace-adapter-contracts/ # cross-adapter fixture and negative-capability tests
  agenttrace-process/           # child process supervision
  agenttrace-collector/         # ingestion helpers and sequencing
  agenttrace-storage/           # SQLite, migrations, events, artifacts, compression, recovery
  agenttrace-redaction/         # built-in + additive secret/path filtering primitives
  agenttrace-registry/          # product-facing adapter registry/capability boundary
  agenttrace-replay/            # dry-run-first allowlisted shell replay
  agenttrace-cli/               # native agenttrace CLI
  agenttrace-server/            # loopback-first local HTTP API + standalone binary
  agenttrace-benchmarks/        # large-trace ingest/load/export/memory/normalization suite
  adapters/*                    # harness-specific collectors/normalizers
fixtures/
  <harness>/                    # sanitized wire-format fixtures
  benchmarks/                   # deterministic large-trace recipe
apps/
  desktop/                      # independent Tauri v2 + React/TypeScript inspector
```

The desktop Tauri crate is intentionally a nested independent Cargo workspace so Tauri's runtime MSRV can move without raising the MSRV of the core Rust workspace.

## Data flow

1. **Detection** identifies an installed harness/version and only the integration modes AgentTrace actually implements for that product surface.
2. **Capability negotiation** enumerates the adapter capability model and returns `native`, `inferred`, `derived`, or explicit `unavailable` evidence for every category. Missing map entries are materialized as unavailable at the registry boundary.
3. **Collection** uses the strongest implemented structured stream, import surface, or conservative process wrapper for the selected harness. Researched upstream hooks/RPC/SDK/watch surfaces are not advertised as implemented until a collector/control path exists.
4. **Normalization** creates schema-v2 `EventEnvelope` records and never upgrades provenance. A deterministic calculation from native data is `derived`, not `native`. The protocol can represent explicit browser navigation/action/network/console evidence, but adapters must not promote generic tool records into `browser.*` events without verified semantics.
5. **Product-field materialization** may promote an upstream field into a typed protocol field only when semantics are sufficiently explicit. The first latency example is Claude Code's native `duration_api_ms`, which the registry converts to `latency_ns` while preserving the original upstream attribute. Other durations are not relabeled as latency.
6. **Redaction** applies built-in rules before normal CLI persistence. An optional additive JSON profile can extend regex patterns, sensitive JSON keys, and sensitive path fragments, but cannot disable built-in safe defaults.
7. **Storage** appends events incrementally to SQLite in sequence order, compresses sufficiently large payloads, maintains run summaries, stores run-scoped artifacts with optional event linkage and SHA-256 identity, and marks interrupted runs during recovery.
8. **Query surfaces** read from the same `TraceStore`: CLI, loopback HTTP API, and Tauri commands. Sharing/read surfaces re-apply the active redaction policy before returning event data and omit raw-source payloads by default where applicable.
9. **Desktop import** uses the product adapter registry rather than a separate frontend parser. A user-selected file path is passed from the native Tauri dialog to Rust, normalized by the selected adapter, redacted, and appended to `TraceStore`.
10. **UI** renders stored evidence and does not synthesize unavailable telemetry client-side. Charts use observed tokens, typed latency, event counts, and durations only; diff, terminal, span, browser, subagent, and context views appear only when corresponding evidence is present.

Library consumers that call lower-level crates directly are responsible for applying the same redaction policy before persistence if they bypass the CLI path.

## Identity and ordering

Each event has a globally unique event ID, run ID, trace ID, sequence number, wall-clock timestamp, and optional span/monotonic timing data. Harness-native identifiers can be preserved in payloads, attributes, or raw-source data.

Sequence is authoritative for local ingestion order. Wall-clock time is useful for human display but is not trusted for strict ordering across processes.

## Protocol boundary

Schema v2 reserves normalized browser event kinds in addition to run/process/model/reasoning/tool/shell/file/Git/MCP/approval/subagent/context/retry/error events. Reserving a canonical event kind makes the evidence representable; it does not imply that every harness or current adapter exposes that signal.

Schema v2 also separates `duration_ns` from `latency_ns`. A duration is not automatically a latency measurement. Typed latency is present only when an implemented source provides explicit latency-like evidence.

Large browser-side evidence such as screenshots, DOM snapshots, or HAR files belongs in artifact storage with an event reference rather than being duplicated through event payloads.

## Capability boundary

Individual adapter crates describe what their normalizers know how to interpret, while `agenttrace-registry` is the product-facing truth boundary. The registry delegates execution/import while filtering researched-yet-unimplemented integration modes, completing every adapter capability report with explicit unavailable evidence, and applying narrowly scoped typed-field materialization where justified by upstream semantics.

That separation prevents a documented upstream feature from accidentally becoming a product claim. For example, a harness may have an SDK, hook, RPC, or filesystem-watch surface upstream while the current AgentTrace implementation supports only a structured stream or import path.

## Raw event preservation

Raw records are useful because upstream schemas evolve. A raw record is not automatically safe to persist. Normal AgentTrace CLI ingestion redacts the complete serialized event before storage, including raw-source JSON. A trace database should still be treated as sensitive engineering data because arbitrary upstream payloads can contain information that no static redaction rule recognizes.

CLI, API, and desktop sharing/read paths run the active redaction rules again. Raw source is omitted by default on sharing-oriented surfaces and must be explicitly requested where supported.

## Process supervision

Process wrapping records process start, stdout/stderr chunks, exit, cancellation, and duration from AgentTrace's own supervisor. It does **not** imply visibility into model requests, tool calls, browser activity, or file reads. Structured child output can add those signals only when the harness explicitly exposes them.

`ProcessSpec` supports an explicit environment map and optional environment clearing. The default supervised harness process inherits the caller's environment so the real harness can run normally; AgentTrace does not record a snapshot of that environment by default. Replay uses a stricter scrubbed environment because replayed historical commands are a different trust boundary.

## Storage strategy

SQLite is the system of record. The current store uses WAL mode, `synchronous=NORMAL`, a bounded connection pool, explicit migrations, indexed run/sequence access, and zstd compression for sufficiently large payloads. Events are committed incrementally so a crash does not require the entire run to be reconstructed from memory.

Artifacts are stored separately from normalized events so large logs, patches, generated reports, screenshots, or other binary/text attachments do not have to be embedded into every event record. Artifact metadata includes run ID, optional event ID, logical name, media type, SHA-256, original byte size, and creation time. Event linkage is validated so an artifact cannot point across runs. Larger artifact bodies use the same compression strategy as event payloads.

Artifact bytes are local-only in the current design. Sanitized JSONL export does not automatically bundle them; the desktop surfaces metadata without exposing bytes by default.

On startup, unfinished runs can be classified as `interrupted` instead of silently appearing successful.

## Adapter contracts

Each built-in adapter is required to declare capability evidence rather than imply parity with another harness. Structured adapters keep sanitized representative fixtures, and cross-adapter tests assert both positive normalization and important negative capabilities. Unknown upstream records should be preserved as raw checkpoints where safe instead of guessed into a richer event kind.

`docs/adapter-authoring.md` defines the contributor contract for command-shape validation, provenance, raw-event preservation, fixtures, and security boundaries.

## Replay safety

Replay only considers recorded `shell.command` events with structured command metadata. A normal replay invocation is a dry run. The plan classifies recognizable side-effect risks such as filesystem mutation, Git mutation, network access, external-service calls, and credential-sensitive arguments. These tags are advisory visibility, not execution authority.

Actual execution requires `--execute` plus an exact command or sequence allowlist. The exact entry confirms only that specific recorded command; AgentTrace deliberately avoids a broad side-effect-category allow switch that would authorize a wider class of operations.

Allowed commands execute in a temporary detached Git worktree at a caller-selected revision, with a scrubbed environment and per-command timeout. This isolates normal repository changes from the caller's active worktree. It is **not an operating-system sandbox**: an allowlisted command still has the current user's OS permissions and may deliberately access resources outside the worktree.

Model calls, tool calls, browser events, MCP operations, approvals, file events, and other trace records are not automatically re-executed.

## Local API boundary

`agenttrace serve` and the standalone `agenttrace-server` binary expose the same server library: health, harness/capability metadata, run summaries, event retrieval, JSONL export, and deterministic run statistics. The server binds to `127.0.0.1:4319` by default and refuses non-loopback binding unless the caller explicitly enables it.

Both server entry points can use the same additive redaction profile as the CLI. The HTTP API is currently read-oriented; recording remains an adapter/CLI responsibility.

## Desktop boundary

The desktop application uses Tauri commands backed directly by `TraceStore` plus the product-facing adapter registry. React/TypeScript owns presentation. The official Tauri dialog plugin is scoped to the main window for user-initiated import file selection.

The current UI provides run history, native adapter-backed import, live active-run polling, dense timeline search/filtering including browser event kinds, complete capability evidence, observed-only token/API-latency/event/duration charts, payload/raw/execution/terminal inspectors, evidence-backed unified and split patch views, span/subagent/context relationships, artifact metadata, deterministic run comparison, sanitized JSONL export, and persistent System/Dark/Light themes. It does not create decorative telemetry or infer hidden relationships when identifiers/evidence are absent.

A separate local HTTP API exists for other clients, but the desktop app does not require a localhost server process to function.

## Performance and validation

`agenttrace-benchmarks` expands a deterministic schema-v2 large-trace recipe into 100,000 events and measures the paths most likely to become bottlenecks: incremental SQLite ingestion, full-run loading, JSONL export, Rust-heap peak deltas during load/export, and adapter normalization throughput. Measurements are emitted for the current machine; they are not converted into hard-coded product claims.

The repository's configured CI covers formatting, clippy, workspace tests, cross-platform checks, desktop web compilation, Windows Tauri compilation, and dependency audit. `docs/testing.md` provides the corresponding end-to-end local validation sequence, including fixtures, replay gates, loopback/remote-bind API behavior, desktop import/themes/latency, packaging, and benchmarks.
