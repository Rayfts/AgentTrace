# AgentTrace trace format

The normalized protocol is versioned independently of the application. Version `1` is represented by `agenttrace-protocol` and serialized as JSON objects for export/interchange.

## Envelope

Every event carries:

- `schema_version`
- `event_id`, `run_id`, `trace_id`
- optional `span_id`, `parent_span_id`
- monotonically increasing per-run `sequence`
- wall-clock `timestamp`
- optional `monotonic_ns` and `duration_ns`
- `harness` and `integration_mode`
- `provenance`
- event `kind`
- normalized `payload`
- optional safely-retained `raw_source`
- optional model, token usage, cost, filesystem, command, and error metadata
- extensible attributes

`provenance.level` is one of `native`, `inferred`, `derived`, or `unavailable`. A normalized event can contain a mix of field origins; `native_fields` and `unavailable_fields` make those boundaries explicit.

## Event kinds

Version 1 reserves these canonical kinds:

```text
run.started             run.completed           run.failed
process.started         process.stdout           process.stderr
process.exited
model.request           model.response           model.usage
reasoning.started       reasoning.completed
tool.call               tool.result
shell.command           shell.output
file.read               file.write               file.create
file.delete             file.patch
git.operation
mcp.request             mcp.response
approval.requested      approval.resolved
subagent.started        subagent.completed
context.added           context.removed          context.compacted
retry                    checkpoint              error
```

Adapters may use attributes and payload fields for upstream-specific detail; they must not mint misleading canonical events. For example, a process wrapper that merely sees terminal text containing `git status` cannot emit `git.operation` as native without a reliable upstream signal. It may keep the terminal text and, if a documented inference rule exists, emit an inferred event with that rule recorded.

## Usage and cost

Token counts are optional integers. Reasoning tokens are separate and remain `null` when the harness/provider does not expose them. Cost is optional and includes currency plus whether the amount was reported by the harness or deterministically computed from a pinned price table. AgentTrace never estimates a monetary value from an unversioned or unknown price.

## Raw source records

A raw source includes the source name, media type, and a JSON value. Exporters may omit raw records by policy. Storage/redaction code is allowed to replace sensitive subtrees with redaction markers while keeping the normalized event.

## Compatibility

Readers must reject unsupported major schema versions rather than silently reinterpret them. Additive optional fields can be introduced within a major version. Meaning changes require a new major schema version and a migration/export compatibility path.
