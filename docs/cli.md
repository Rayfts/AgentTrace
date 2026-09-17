# AgentTrace CLI

The `agenttrace` binary records and inspects normalized local traces. Commands print machine-readable JSON unless the command is itself streaming JSONL.

## Database

By default the CLI stores traces in `.agenttrace/agenttrace.db` under the current working directory. Override it globally with `--db`:

```bash
agenttrace --db /path/to/agenttrace.db inspect <run-id>
```

## Discover harnesses

```bash
agenttrace harnesses
agenttrace capabilities
agenttrace capabilities codex
agenttrace doctor
```

`harnesses` checks whether each built-in adapter can find its expected executable. `capabilities` reports the evidence level behind every telemetry class instead of implying every harness exposes the same data.

## Record a run

```bash
agenttrace run --harness codex -- codex exec "fix the failing test"
```

Everything after `--` is passed to the selected harness adapter. An adapter may reject a command that does not match a real supported integration mode. For example, AgentTrace does not silently convert an interactive UI into a fictional headless interface.

Normalized events are streamed to stdout as JSONL and persisted after redaction.

## Import an existing trace

```bash
agenttrace import --harness codex ./codex-run.jsonl
```

Import support is harness-specific. The adapter normalizes the source file into the common AgentTrace protocol while preserving safe raw-source evidence when available.

## Inspect

```bash
agenttrace inspect <run-id>
agenttrace inspect <run-id> --raw
```

Raw-source data is hidden by default in `inspect` output. `--raw` exposes the already-redacted raw-source payload stored with each event.

## Export

```bash
agenttrace export <run-id>
agenttrace export <run-id> --output trace.jsonl
```

The export format is newline-delimited normalized `EventEnvelope` JSON.

## Compare

```bash
agenttrace compare <left-run-id> <right-run-id>
```

Comparison is deterministic. It reports event counts by kind and provenance, observed token totals, reported/deterministic cost partitioned by currency, and summed event durations. It does not ask a model to decide which run is better.

## Replay

Replay is dry-run-first:

```bash
agenttrace replay <run-id> --repo /path/to/repository
```

The dry run lists only recorded `shell.command` events that are eligible for replay. To execute, explicitly allow exact commands or event sequence numbers:

```bash
agenttrace replay <run-id> \
  --repo /path/to/repository \
  --execute \
  --allow "cargo test" \
  --allow-sequence 42
```

Additional controls:

```text
--revision <git-revision>       detached worktree revision, default HEAD
--timeout-seconds <seconds>     per-command timeout, default 300
--continue-on-error             continue after a replayed command fails
```

Replay uses a detached Git worktree and a scrubbed environment. It is not an OS sandbox; see [`replay.md`](replay.md).

## Local API

The local API is a separate binary:

```bash
agenttrace-server
agenttrace-server --db .agenttrace/agenttrace.db
agenttrace-server --bind 127.0.0.1:4319
```

The server refuses non-loopback binds unless `--allow-remote` is explicitly supplied.
