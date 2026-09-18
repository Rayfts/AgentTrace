# Security and privacy model

AgentTrace observes developer tools, so a trace can contain credentials, private source code, prompts, customer data, command output, filesystem paths, or stored artifacts. Security defaults are therefore part of the trace pipeline, not an export-only afterthought.

## Current defaults

- AgentTrace storage is local SQLite. The project does not require a cloud account and does not implement automatic trace upload.
- AgentTrace does **not record a snapshot of the caller's environment by default**. The redaction crate also provides a deny-by-default `EnvironmentPolicy` helper for explicitly selected environment values.
- Normal supervised harness processes inherit the caller's environment unless an adapter explicitly asks `ProcessSpec` to clear it. This preserves the real harness execution environment; inheritance is different from recording those variables into the trace.
- The normal CLI run/import path serializes each normalized event, applies recursive secret/path redaction, then persists the redacted event.
- Known secret patterns and sensitive JSON keys are replaced with `[REDACTED]` by the built-in redactor.
- Strings that resolve to configured sensitive path components such as `.ssh`, `.aws`, `.gnupg`, `.kube`, and `.env` are redacted by the normal CLI persistence path. This is not a general filesystem access-control mechanism.
- CLI process supervision invokes executables through structured program/argument fields. Harness-specific adapters may legitimately request a shell when that is the upstream interface they are tracing.
- CLI, HTTP, and desktop sharing/read surfaces re-apply the active redaction policy before returning event payloads. Raw-source payloads are excluded by default on CLI/HTTP export paths.

Static redaction cannot guarantee removal of every secret format. A harness can emit arbitrary source code, prompts, file contents, tokens in unknown formats, or proprietary data. Treat trace databases, stored artifacts, JSONL exports, screenshots, and raw-source views as sensitive engineering artifacts even after redaction.

## Additive custom redaction profiles

Organizations can extend the built-in policy with a JSON profile. The main CLI and standalone server accept `--redaction-config <path>`; the desktop shell reads `AGENTTRACE_REDACTION_CONFIG` when set.

A profile can add:

- regular-expression patterns;
- sensitive JSON key names;
- sensitive path fragments/components.

Example:

```json
{
  "patterns": [
    {"name": "internal_ticket", "regex": "AT-[0-9]{6}"}
  ],
  "sensitive_json_keys": ["customer_reference"],
  "sensitive_path_fragments": [".agenttrace-private"]
}
```

Profiles are additive: they cannot remove built-in safe rules. Unknown fields and invalid regexes are rejected instead of being silently ignored. A newer profile can therefore re-sanitize stored events at inspect/export/API time, but it cannot recover data that was already replaced with `[REDACTED]` during ingestion.

## Provenance is a security feature

AgentTrace does not infer secret-bearing context merely to make a trace appear complete. If a harness does not expose model request bodies or context contents, those fields are marked unavailable rather than reconstructed through side channels. Product-facing capability reports enumerate all categories, so an omitted capability cannot be mistaken for an affirmative observation.

## Raw payload warning

Native structured streams can contain more than the visible harness UI. A raw event may include tool inputs, model messages, file contents, headers, extension state, or command output. Raw-source records stored through the normal CLI path are redacted before persistence, but they can still contain sensitive information not matched by known rules.

The CLI hides raw-source data in `inspect` and `export` output unless `--raw` is supplied. The HTTP API hides raw-source data unless `?raw=true` is supplied. Both surfaces re-run the active redaction rules before returning data. The desktop inspector has an explicit local raw-source visibility control and uses the active desktop redaction profile before returning events to React.

## Artifacts

The storage layer supports run-scoped artifacts with optional event linkage, SHA-256 content identity, size metadata, and zstd compression for large payloads. Artifact bytes are local database content and must be treated as sensitive. AgentTrace currently exposes artifact **metadata** to the desktop inspector by default; it does not automatically upload artifacts or include arbitrary artifact bytes in sanitized JSONL exports.

Library consumers that call `TraceStore::store_artifact` directly are responsible for deciding whether the artifact bytes require additional domain-specific sanitization before storage. Hashing and compression are integrity/storage mechanisms, not redaction or encryption.

## Replay

Replay is dry-run-first and only considers recorded `shell.command` events. Execution requires `--execute` plus an exact command or sequence allowlist.

Allowed commands execute in a temporary detached Git worktree, with a scrubbed environment and a per-command timeout. This protects normal repository state from ordinary replayed file changes, but it is **not an OS sandbox**. An allowlisted command still runs as the current user and can deliberately access paths, processes, sockets, or network resources outside the worktree if the operating system allows it.

Imported traces never grant replay permission automatically. Redacted command text is not reconstructed for replay.

## Local API

`agenttrace serve` and the standalone `agenttrace-server` binary bind to loopback by default. A non-loopback bind is rejected unless `--allow-remote` is explicitly supplied. AgentTrace does not currently provide authentication or TLS for the local API; remote exposure therefore requires an external trusted boundary and should not be enabled casually.

## Encryption

Application-level encrypted trace storage is not implemented. Filesystem- or volume-level encryption can protect an AgentTrace database at rest today. AgentTrace should not claim application-level encryption until key storage, rotation, recovery, and cross-platform behavior are implemented and tested.

## Reporting vulnerabilities

Do not publish vulnerabilities that could expose credentials, source code, or private trace data. Follow the private reporting path in [`../SECURITY.md`](../SECURITY.md).
