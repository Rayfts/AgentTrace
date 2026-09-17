# AgentTrace

AgentTrace is a local-first debugger, recorder, profiler, and observability platform for AI coding agents.

When an agent changes a repository, AgentTrace records the evidence needed to inspect **what actually happened**: tool activity, shell commands, terminal output, file changes, model-visible usage data, failures, retries, MCP activity, timing, raw harness events, and other telemetry that the harness genuinely exposes.

AgentTrace does not force ten different agent runtimes into a fictional common API. Each adapter uses the strongest legitimate integration surface available for that harness, and the normalized protocol records how trustworthy every observation is.

> **Status:** pre-1.0. The core protocol, storage, process supervision, redaction, ten initial adapters, CLI, replay engine, local API, and Tauri/React inspector are implemented on the current development line. Compatibility may still change before the first stable release.

## Telemetry truth model

Every normalized event carries provenance:

- **native** — emitted directly by the harness or a documented integration surface;
- **inferred** — observed from a side effect with an explicit inference rule;
- **derived** — deterministically calculated from recorded evidence;
- **unavailable** — not exposed reliably enough to claim.

AgentTrace never upgrades missing telemetry into a guess. Raw source records are retained when safe so normalization can improve later without pretending an older collector observed something it did not.

## Supported harnesses

The initial adapter registry contains:

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

Integration modes differ by harness. Structured streams are preferred where they exist; documented hooks, session import, storage observation, or conservative process wrapping are used elsewhere. See [`docs/harnesses.md`](docs/harnesses.md) for the evidence-backed capability matrix.

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

## Quick start

Check installed harnesses and their telemetry capabilities:

```bash
agenttrace harnesses
agenttrace capabilities codex
agenttrace doctor
```

Record a supported harness run:

```bash
agenttrace run --harness codex -- codex exec "fix the failing test"
```

Inspect, export, and compare traces:

```bash
agenttrace inspect <run-id>
agenttrace export <run-id> --output trace.jsonl
agenttrace compare <left-run-id> <right-run-id>
```

Import a supported existing trace/session format:

```bash
agenttrace import --harness codex ./codex-run.jsonl
```

The default CLI database is `.agenttrace/agenttrace.db` under the current working directory. Use the global `--db` option to point multiple commands at the same database explicitly.

Full CLI documentation: [`docs/cli.md`](docs/cli.md).

## Replay

Replay is deliberately narrower than “rerun the agent.” Only recorded `shell.command` events are eligible, and a normal replay command is a dry run:

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

Replay runs in a temporary detached Git worktree with a scrubbed environment and a per-command timeout. This protects normal repository state, but it is **not an operating-system sandbox**; allowlisted commands still run with the current user's OS permissions. See [`docs/replay.md`](docs/replay.md).

## Local API

Run the loopback-first API against the same SQLite database:

```bash
agenttrace-server --db .agenttrace/agenttrace.db
```

The default bind is `127.0.0.1:4319`. Non-loopback binding is rejected unless `--allow-remote` is explicitly supplied.

Current endpoints include health, harness detection, capability evidence, run listing/detail, event retrieval, JSONL export, and deterministic run statistics.

## Desktop inspector

`apps/desktop` contains a Tauri v2 + React inspector with a high-density DevTools-style timeline, run browser, live refresh, search/category filters, provenance badges, payload/raw/execution panels, and observed usage/cost/error/retry metrics.

```bash
cd apps/desktop
npm install
npm run tauri dev
```

Set `AGENTTRACE_DB` before launch to inspect an existing CLI database. The desktop Tauri crate is intentionally isolated from the core workspace so its newer runtime MSRV does not raise the core AgentTrace MSRV. See [`docs/desktop.md`](docs/desktop.md).

## Architecture

The Rust core is split into small crates for the protocol, adapter contract, process supervision, collection, storage, redaction, adapter registry, replay, CLI, and local API. Harness-specific adapters live under `crates/adapters/`.

Key design documents:

- [`docs/architecture.md`](docs/architecture.md) — system boundaries and data flow;
- [`docs/trace-format.md`](docs/trace-format.md) — normalized event schema;
- [`docs/harnesses.md`](docs/harnesses.md) — adapter capability matrix;
- [`docs/research/reference-adapters.md`](docs/research/reference-adapters.md) — upstream integration research;
- [`docs/security.md`](docs/security.md) — data-handling design;
- [`SECURITY.md`](SECURITY.md) — vulnerability reporting and security boundaries.

## Performance benchmark

A reproducible storage benchmark exercises the same SQLite append/load path as normal traces:

```bash
cargo run --release -p agenttrace-storage --example ingest_benchmark -- 10000
```

It reports measured write/read duration and events per second for the current machine. No benchmark number is hard-coded into project claims.

## Development

Core quality gates:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets
```

CI runs the core checks across Linux, macOS, and Windows, builds the React frontend, checks the Tauri shell on Windows, and runs a Rust dependency audit. Tagged `v*` releases build `agenttrace` and `agenttrace-server` archives for Linux, macOS, and Windows with SHA-256 checksum files.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) before changing adapters or telemetry claims.

## Security and privacy

Redaction is applied before normal persistence, environment capture is deny-by-default, the API is loopback-first, and replay requires explicit execution permission plus allowlisting. Traces can still contain sensitive engineering data; treat trace databases and exports as local sensitive artifacts.

## License

MIT. See [`LICENSE`](LICENSE).
