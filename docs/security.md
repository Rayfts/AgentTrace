# Security and privacy model

AgentTrace observes developer tools, so a trace can contain credentials, private source code, prompts, customer data, command output, or filesystem paths. Security defaults are therefore part of the trace pipeline, not an export-only afterthought.

## Current defaults

- AgentTrace storage is local SQLite. The project does not require a cloud account and does not implement automatic trace upload.
- AgentTrace does **not record a snapshot of the caller's environment by default**. The redaction crate also provides a deny-by-default `EnvironmentPolicy` helper for explicitly selected environment values.
- Normal supervised harness processes inherit the caller's environment unless an adapter explicitly asks `ProcessSpec` to clear it. This preserves the real harness execution environment; inheritance is different from recording those variables into the trace.
- The normal CLI run/import path serializes each normalized event, applies recursive secret/path redaction, then persists the redacted event.
- Known secret patterns and sensitive JSON keys are replaced with `[REDACTED]` by the built-in redactor.
- Strings that resolve to configured sensitive path components such as `.ssh`, `.aws`, `.gnupg`, `.kube`, and `.env` are redacted by the CLI persistence path. This is not a general filesystem access-control mechanism.
- CLI process supervision invokes executables through structured program/argument fields. Harness-specific adapters may legitimately request a shell when that is the upstream interface they are tracing.
- CLI and HTTP exports re-apply the current built-in redaction policy before serialization. Raw-source payloads are excluded by default and require an explicit `--raw` or `?raw=true` request.

Static redaction cannot guarantee removal of every secret format. A harness can emit arbitrary source code, prompts, file contents, tokens in unknown formats, or proprietary data. Treat trace databases, JSONL exports, screenshots, and raw-source views as sensitive engineering artifacts even after redaction.

## Provenance is a security feature

AgentTrace does not infer secret-bearing context merely to make a trace appear complete. If a harness does not expose model request bodies or context contents, those fields are marked unavailable rather than reconstructed through side channels.

## Raw payload warning

Native structured streams can contain more than the visible harness UI. A raw event may include tool inputs, model messages, file contents, headers, extension state, or command output. Raw-source records stored through the CLI are redacted before persistence, but they can still contain sensitive information not matched by built-in rules.

The CLI hides raw-source data in `inspect` and `export` output unless `--raw` is supplied. The HTTP API hides raw-source data unless `?raw=true` is supplied. Both export surfaces re-run current redaction rules before returning data. The desktop inspector has an explicit raw-source visibility control for the local database.

## Replay

Replay is dry-run-first and only considers recorded `shell.command` events. Execution requires `--execute` plus an exact command or sequence allowlist.

Allowed commands execute in a temporary detached Git worktree, with a scrubbed environment and a per-command timeout. This protects normal repository state from ordinary replayed file changes, but it is **not an OS sandbox**. An allowlisted command still runs as the current user and can deliberately access paths, processes, sockets, or network resources outside the worktree if the operating system allows it.

Imported traces never grant replay permission automatically.

## Local API

`agenttrace serve` and the standalone `agenttrace-server` binary bind to loopback by default. A non-loopback bind is rejected unless `--allow-remote` is explicitly supplied. AgentTrace does not currently provide authentication or TLS for the local API; remote exposure therefore requires an external trusted boundary and should not be enabled casually.

## Encryption

Application-level encrypted trace storage is not implemented. Filesystem- or volume-level encryption can protect an AgentTrace database at rest today. AgentTrace should not claim application-level encryption until key storage, rotation, recovery, and cross-platform behavior are implemented and tested.

## Reporting vulnerabilities

Do not publish vulnerabilities that could expose credentials, source code, or private trace data. Follow the private reporting path in [`../SECURITY.md`](../SECURITY.md).
