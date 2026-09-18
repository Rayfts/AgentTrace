# AgentTrace trace format

The normalized protocol is versioned independently of the application. Version `2` is represented by `agenttrace-protocol` and serialized as JSON objects for export/interchange.

Schema v2 adds a canonical browser-event namespace and a typed optional `latency_ns` field. No adapter is allowed to emit a `browser.*` event merely because a generic tool name looks browser-related, and latency remains absent unless an upstream surface exposes a semantically explicit latency measurement.

## Envelope

Every event carries:

- `schema_version`
- `event_id`, `run_id`, `trace_id`
- optional `span_id`, `parent_span_id`
- monotonically increasing per-run `sequence`
- wall-clock `timestamp`
- optional `monotonic_ns`, `duration_ns`, and `latency_ns`
- `harness` and `integration_mode`
- `provenance`
- event `kind`
- normalized `payload`
- optional safely-retained `raw_source`
- optional model, token usage, cost, filesystem, command, and error metadata
- extensible attributes

`duration_ns` represents an observed duration for the event/run surface that supplied it. `latency_ns` is deliberately separate: it is populated only when the upstream harness exposes a value whose semantics are explicitly latency-like rather than simply total elapsed duration. For example, Claude Code's native `duration_api_ms` result field is deterministically promoted to `latency_ns` while the original upstream attribute remains preserved.

`provenance.level` is one of `native`, `inferred`, `derived`, or `unavailable`. A normalized event can contain a mix of field origins; `native_fields` and `unavailable_fields` make those boundaries explicit.

## Event kinds

Version 2 reserves these canonical kinds:

```text
run.started             run.completed           run.failed
process.started         process.stdout          process.stderr
process.exited
model.request           model.response          model.usage
reasoning.started       reasoning.completed
tool.call               tool.result
shell.command           shell.output
file.read               file.write              file.create
file.delete             file.patch
git.operation
browser.navigation      browser.action           browser.network
browser.console
mcp.request             mcp.response
approval.requested      approval.resolved
subagent.started        subagent.completed
context.added           context.removed          context.compacted
retry                   checkpoint               error
```

Adapters may use attributes and payload fields for upstream-specific detail; they must not mint misleading canonical events. For example, a process wrapper that merely sees terminal text containing `git status` cannot emit `git.operation` as native without a reliable upstream signal. Likewise, a generic tool call named `browser` is not enough to claim `browser.navigation` or `browser.network` unless the adapter has verified the upstream record shape and semantic meaning.

## Browser events

Browser events are intentionally small and transport-neutral:

- `browser.navigation` — explicit page/navigation lifecycle evidence;
- `browser.action` — explicit browser interaction such as click/type/evaluate when the upstream harness identifies it as browser activity;
- `browser.network` — explicit browser-scoped request/response/network evidence;
- `browser.console` — explicit browser console/log/error evidence.

Screenshots, DOM snapshots, HAR files, and similar larger data should normally be stored as run artifacts and referenced from event payload/attributes rather than embedded repeatedly into the event stream.

## Usage and cost

Token counts are optional integers. Reasoning tokens are separate and remain absent when the harness/provider does not expose them. Cost is optional and includes currency plus whether the amount was reported by the harness or deterministically computed from a pinned price table. AgentTrace never estimates a monetary value from an unversioned or unknown price.

## Raw source records

A raw source includes the source name, media type, and a JSON value. Exporters may omit raw records by policy. Storage/redaction code is allowed to replace sensitive subtrees with redaction markers while keeping the normalized event.

## Compatibility

`schema_version` is persisted and exported with every event so consumers can make compatibility decisions explicitly. Schema v2 is the current development format. The repository is pre-1.0, so the v1-to-v2 browser/latency additions are being made before a stable compatibility promise; future meaning changes must include an explicit schema-version change and a documented migration or interchange path rather than silently reinterpreting an old field.
