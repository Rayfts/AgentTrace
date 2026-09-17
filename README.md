# AgentTrace

AgentTrace is a local-first debugger, recorder, profiler, and observability platform for AI coding agents.

When an agent changes a repository, AgentTrace records the evidence needed to inspect **what actually happened**: model interactions where exposed, tool activity, shell commands, terminal output, file changes, MCP activity, subagents, context changes, retries, failures, timing, token/cost samples, raw harness events, and stored artifacts.

AgentTrace does not force ten different runtimes into a fictional common API. Product-facing capability reports enumerate all supported telemetry categories and mark each one `native`, `inferred`, `derived`, or `unavailable`. Researched upstream integration surfaces are not advertised as implemented until AgentTrace has a real collector/control path for them.

> **Status:** pre-1.0. The core protocol, local storage, artifacts, redaction profiles, ten initial adapters, CLI, replay engine, local API, adapter contract fixtures, benchmark suite, and Tauri/React inspector are implemented on the current development line. Compatibility may still change before the first stable release.

## Telemetry truth model

Every normalized event carries provenance:

- **native** — emitted directly by the harness or a documented integration surface;
- **inferred** — observed from a side effect with an explicit inference rule;
- **derived** — deterministically calculated from recorded evidence;
- **unavailable** — not exposed reliably enough to claim.

AgentTrace never upgrades missing telemetry into a guess. Unknown structured records are preserved conservatively where safe so normalization can improve later without pretending an older collector observed something it did not.

## Supported harnesses

The initial product registry contains:

- OpenAI Codex
- Claude Code
- OpenCode
- Pi
- Gemini CLI
- Aider
- Goose
- Cline
- Roo Code
- Continue

Integration modes differ by harness. Structured streams are preferred where implemented; imports and conservative process wrapping are used where that is the strongest implemented path. See [`docs/harnesses.md`](docs/harnesses.md) for the evidence-backed matrix and researched follow-on surfaces.

## Build

The core workspace requires Rust 1.85 or newer.

```bash
cargo build --workspace
```

Install the command-line binaries directly from the repository:

```bash
cargo install --path crates/agenttrace-cli
cargo install --path crates/agenttrace-server
```

`agenttrace serve` is the primary local-API workflow. The standalone `agenttrace-server` binary remains available for service-oriented deployments and packaging.

## Quick start

```bash
agenttrace harnesses
agenttrace capabilities codex
agenttrace doctor
agenttrace run --harness codex -- codex exec "fix the failing test"
agenttrace inspect <run-id>
agenttrace export <run-id> --output trace.jsonl
agenttrace compare <left-run-id> <right-run-id>
```

Export re-applies the active redaction policy. Raw-source payloads are omitted by default; include raw source explicitly only when needed:

```bash
agenttrace export <run-id> --raw --output trace-with-raw.jsonl
```

Import supported existing traces/sessions:

```bash
agenttrace import --harness codex ./codex-run.jsonl
```

The default database is `.agenttrace/agenttrace.db`. Use global `--db` to point commands at another database.

## Custom redaction

Built-in safe rules are always enabled. Add organization-specific patterns, JSON keys, and sensitive path fragments with an additive JSON profile:

```bash
agenttrace --redaction-config ./agenttrace-redaction.json \
  run --harness codex -- codex exec "fix the failing test"
```

```json
{
  "patterns": [
    {"name": "internal_ticket", "regex": "AT-[0-9]{6}"}
  ],
  "sensitive_json_keys": ["customer_reference"],
  "sensitive_path_fragments": [".agenttrace-private"]
}
```

The same profile can be used for inspection/export/API serving so newer rules can be re-applied to stored events. See [`docs/cli.md`](docs/cli.md) and [`docs/security.md`](docs/security.md).

## Replay

Replay is deliberately narrower than “rerun the agent.” Only recorded `shell.command` events are eligible, and replay is dry-run-first:

```bash
agenttrace replay <run-id> --repo /path/to/repository
```

Execution requires `--execute` plus an exact command or sequence allowlist:

```bash
agenttrace replay <run-id> \
  --repo /path/to/repository \
  --execute \
  --allow "cargo test"
```

Replay runs in a temporary detached Git worktree with a scrubbed environment and a per-command timeout. This protects normal repository state, but it is **not an operating-system sandbox**. See [`docs/replay.md`](docs/replay.md).

## Local API

```bash
agenttrace --db .agenttrace/agenttrace.db serve
```

The standalone binary exposes the same server library:

```bash
agenttrace-server --db .agenttrace/agenttrace.db
```

The default bind is `127.0.0.1:4319`. Non-loopback binding is rejected unless `--allow-remote` is explicit. Event/export endpoints re-apply the active redaction policy and hide raw source by default.

Current endpoints include health, harness detection, complete capability evidence, run listing/detail, event retrieval, JSONL export, and deterministic run statistics.

## Storage and artifacts

SQLite is the local system of record. Event ingestion is incremental, WAL-backed, indexed, migration-driven, and zstd-compresses sufficiently large event payloads.

Run-scoped artifacts can be stored with optional event linkage, SHA-256 content identity, original size, media type, and compression metadata. Artifact bytes remain local and are not automatically uploaded or included in sanitized JSONL exports.

## Desktop inspector

`apps/desktop` contains a Tauri v2 + React DevTools-style inspector with:

- run history and live active-run refresh;
- searchable/filterable timeline;
- complete capability evidence indicators;
- payload, raw, execution, terminal, diff, and relations inspectors;
- unified and side-by-side diff rendering only when actual patch text is exposed;
- span/subagent/context views only when corresponding trace evidence exists;
- deterministic run comparison;
- sanitized JSONL export;
- artifact metadata;
- observed usage/cost/retry/error fields without fake precision.

```bash
cd apps/desktop
npm install
npm run tauri dev
```

Set `AGENTTRACE_DB` to inspect an existing CLI database and optionally `AGENTTRACE_REDACTION_CONFIG` for an additive desktop redaction profile. The Tauri crate is isolated from the core workspace so its newer runtime MSRV does not raise the core MSRV. See [`docs/desktop.md`](docs/desktop.md).

## Architecture

The Rust core is split into small crates for the protocol, adapter contract, process supervision, collection, storage/artifacts, redaction, adapter registry, replay, CLI, local API, cross-adapter contracts, and benchmarks. Harness adapters live under `crates/adapters/`.

Key documents:

- [`docs/architecture.md`](docs/architecture.md)
- [`docs/trace-format.md`](docs/trace-format.md)
- [`docs/adapter-authoring.md`](docs/adapter-authoring.md)
- [`docs/harnesses.md`](docs/harnesses.md)
- [`docs/research/reference-adapters.md`](docs/research/reference-adapters.md)
- [`docs/security.md`](docs/security.md)
- [`SECURITY.md`](SECURITY.md)

## Performance benchmarks

The deterministic large-trace suite expands `fixtures/benchmarks/large-trace.json` into 100,000 events and measures ingestion, full-trace loading, JSONL export, Rust-heap deltas, and Codex normalization throughput:

```bash
cargo run --release -p agenttrace-benchmarks
```

A smaller storage microbenchmark is also available:

```bash
cargo run --release -p agenttrace-storage --example ingest_benchmark -- 10000
```

Benchmarks print measurements for the current machine; AgentTrace does not hard-code performance claims from one environment.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets
```

CI is configured to run core checks across Linux/macOS/Windows, build the React frontend, check the Tauri shell on Windows, and run dependency auditing. Tagged `v*` releases package `agenttrace` and `agenttrace-server` for Linux, macOS, and Windows with SHA-256 checksum files.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) before changing adapters or telemetry claims.

## Security and privacy

Redaction is applied before normal event persistence and again on sharing/read surfaces; custom profiles can only add protections. Environment capture is deny-by-default, the API is loopback-first, replay is allowlisted, and no automatic cloud upload exists. Trace databases, artifacts, exports, and screenshots can still contain sensitive engineering data.

Application-level encrypted storage is not currently implemented; see [`docs/security.md`](docs/security.md).

## License

MIT. See [`LICENSE`](LICENSE).
