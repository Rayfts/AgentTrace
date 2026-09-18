# Adapter authoring

AgentTrace adapters are evidence translators, not compatibility shims. An adapter should expose only telemetry that a harness actually provides through a documented or otherwise verifiable integration surface.

## Contract

Implement `HarnessAdapter` from `agenttrace-adapter-api`. An adapter identifies its harness, detects the local installation, reports an evidence-backed capability matrix, starts only legitimate run modes, streams normalized events, supports cancellation when it owns a process, and imports persisted traces only when the source format is understood.

The current trait deliberately separates capability reporting from execution. A harness can therefore support `SessionImport` or `FilesystemWatch` while rejecting `ProcessWrap` instead of pretending it has a headless CLI.

## Research before code

Before adding or changing an adapter:

1. Inspect the official upstream repository and current documentation.
2. Identify the strongest stable integration surface: structured stdout, hooks, session files, extension APIs, RPC/SDK, MCP, or conservative process wrapping.
3. Record the exact upstream command/event/session shape in `docs/harnesses.md` or a focused research note.
4. Avoid undocumented flags and private APIs when a public mechanism exists.
5. If the upstream behavior is version-dependent, keep detection conservative and reject incompatible explicit output modes rather than guessing.

## Provenance rules

Every normalized event carries `Provenance`:

- `native`: directly emitted by the harness or directly observed by AgentTrace's own process supervisor;
- `inferred`: classified from an observable side effect under an explicit rule;
- `derived`: deterministically calculated from other recorded evidence;
- `unavailable`: capability reporting only; never manufacture an event for missing data.

A generic `tool.call` must not become `shell.command`, `file.write`, `mcp.request`, or `git.operation` unless the adapter has enough upstream identity/metadata to make that classification safely.

Unknown structured records should normally be preserved as `checkpoint` events with their raw source attached. Dropping unknown records makes later normalization improvements impossible.

## Raw source preservation

Use the helpers in `adapters/common` for native structured records. Raw source data is useful for schema evolution, but it still passes through AgentTrace redaction before durable persistence in CLI workflows.

Never add fixture data containing real credentials, private source code, home-directory identifiers, customer data, or real user prompts.

## Capabilities

`capabilities()` is a product contract. It must match what the current adapter actually normalizes, not everything the upstream harness could theoretically expose.

For each capability, provide:

- provenance level;
- source surface;
- a limitation note when the claim is narrower than the capability name.

When implementation lags upstream capability, report the missing signal as unavailable or omit the positive claim and document the gap.

## Structured-process adapters

For JSON/JSONL subprocesses, prefer `StructuredRunRegistry` from `adapters/common`:

- it supervises the child process;
- emits native `process.*` lifecycle records;
- passes line records to a harness-specific normalizer;
- retains cancellation ownership;
- assigns a stable trace ID and monotonic sequence within the collected stream.

Command builders must validate the invocation shape they know how to trace. Examples in the current tree include requiring `codex exec`, `goose run`, and `cn -p` rather than silently rewriting unrelated interactive commands.

## Import-only adapters

An import adapter should validate the file shape before emitting events. Unsupported direct execution must return `AdapterError::Unsupported` rather than spawning an invented process interface.

Roo Code is the reference case: its adapter imports persisted task JSON and intentionally rejects process wrapping.

## Fixture-driven tests

Every structured adapter should have sanitized representative fixture data under `fixtures/<harness>/` where practical. Tests should assert both positive and negative behavior:

- known lifecycle events normalize correctly;
- usage/cost is attached only when upstream fields exist;
- unknown records survive as checkpoints/raw data;
- missing capabilities remain missing;
- command builders reject incompatible output modes;
- sequence numbers remain ordered.

Contributors should be able to test normalization without installing all supported harnesses.

## Review checklist

Before an adapter change is ready:

- upstream source/documentation evidence is recorded;
- capability report matches the code;
- fixture data is sanitized;
- raw source is preserved where safe;
- no undocumented CLI flags were introduced;
- the adapter does not force permissive approval modes;
- `cargo fmt`, clippy, and tests pass;
- `docs/harnesses.md` is updated if the observable surface changed.
