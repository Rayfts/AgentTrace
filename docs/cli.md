# AgentTrace CLI

The `agenttrace` binary records and inspects normalized local traces. Commands print machine-readable JSON unless the command is itself streaming JSONL.

## Database

By default the CLI stores traces in `.agenttrace/agenttrace.db` under the current working directory. Override it globally with `--db`:

```bash
agenttrace --db /path/to/agenttrace.db inspect <run-id>
```

## Redaction profile

Built-in safe redaction rules are always enabled. Add project-specific patterns, JSON keys, and sensitive path components with the global `--redaction-config` option:

```bash
agenttrace --redaction-config ./agenttrace-redaction.json \
  run --harness codex -- codex exec "fix the failing test"
```

Example additive profile:

```json
{
  "patterns": [
    {"name": "internal_ticket", "regex": "AT-[0-9]{6}"}
  ],
  "sensitive_json_keys": ["customer_reference"],
  "sensitive_path_fragments": [".agenttrace-private"]
}
```

Profiles cannot disable the built-in rules. Unknown profile fields and invalid regular expressions are rejected. The same profile can be used with `inspect`, `export`, or `serve` to apply newer organization-specific rules to already-redacted stored events before they are displayed or shared.

## Discover harnesses

```bash
agenttrace harnesses
agenttrace capabilities
agenttrace capabilities codex
agenttrace doctor
```

`harnesses` checks whether each built-in adapter can find its expected executable. `capabilities` reports all 21 telemetry categories with an evidence level instead of implying every harness exposes the same data. The product registry also removes researched integration modes that do not yet have a real AgentTrace implementation.

## Record a run

```bash
agenttrace run --harness codex -- codex exec "fix the failing test"
```

Everything after `--` is passed to the selected harness adapter. An adapter may reject a command that does not match a real supported integration mode. AgentTrace does not silently convert an interactive UI into a fictional headless interface.

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

Raw-source data is hidden by default. `inspect` re-applies the current redaction policy before output; `--raw` exposes the raw-source payload only after that pass.

## Export

```bash
agenttrace export <run-id>
agenttrace export <run-id> --output trace.jsonl
agenttrace export <run-id> --raw --output trace-with-raw.jsonl
```

The export format is newline-delimited normalized `EventEnvelope` JSON. Export re-applies the **current** built-in and optional custom redaction policy even though normal run/import persistence is already redacted. Raw-source payloads are excluded by default; `--raw` includes them only after the same export-time redaction pass.

Static redaction cannot guarantee that arbitrary proprietary data or unknown secret formats are removed. Review exports before sharing them outside your trusted boundary.

## Compare

```bash
agenttrace compare <left-run-id> <right-run-id>
```

Comparison is deterministic. It reports event counts by kind/provenance plus sums of recorded usage and cost samples partitioned by currency. Those aggregates are measurements over recorded events, not a claim that every harness reports per-turn rather than cumulative usage. It does not ask a model to decide which run is better.

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

Replay uses a detached Git worktree and a scrubbed environment. It is not an OS sandbox; see [`replay.md`](replay.md). A global redaction profile is parsed for normal CLI consistency but replay executes only the command evidence already stored in the trace; it does not reconstruct hidden/redacted command text.

## Local API

The primary workflow is built into the main CLI:

```bash
agenttrace serve
agenttrace --db .agenttrace/agenttrace.db serve
agenttrace --redaction-config ./agenttrace-redaction.json serve
agenttrace serve --bind 127.0.0.1:4319
```

A standalone `agenttrace-server` binary is also shipped for service-oriented deployments and uses the same Rust server implementation. It supports the same `--redaction-config` profile path.

The server refuses non-loopback binds unless `--allow-remote` is explicitly supplied. Event and export endpoints hide raw-source payloads by default and re-apply the active redaction policy before returning trace data. Add `?raw=true` only when raw-source evidence is intentionally required.
