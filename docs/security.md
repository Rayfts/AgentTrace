# Security and privacy model

AgentTrace observes developer tools, so a trace can contain credentials, private source code, prompts, customer data, command output, or filesystem paths. Security defaults are therefore part of the trace protocol, not an export-only afterthought.

## Defaults

- Local-only storage. No automatic upload or remote telemetry from AgentTrace.
- Environment variables are **not captured by default**. A user or adapter must explicitly allow individual variable names, and values still pass through redaction.
- Raw harness payloads pass through redaction before durable storage when raw retention is enabled.
- Known secret patterns and sensitive JSON keys are replaced with `[REDACTED]`.
- Sensitive paths such as `.ssh`, `.aws`, `.gnupg`, `.kube`, and `.env` are excluded from content capture by default. Metadata policy can be configured separately from content capture.
- CLI process supervision invokes executables directly; AgentTrace does not concatenate untrusted arguments into a shell command.
- Export sanitization runs a second redaction pass because redaction rules can change after ingestion.

## Provenance is a security feature

AgentTrace does not infer secret-bearing context merely to make a trace appear complete. If a harness does not expose the model request or context contents, those fields are unavailable. Side-channel reconstruction of prompts or credentials is out of scope.

## Raw payload warning

Native structured streams can contain more than the visible UI. A raw event may include tool inputs, model messages, file contents, headers, or extension data. The desktop UI and CLI must show a clear raw-payload warning and make raw export opt-in for sanitized sharing.

## Replay

Replay is denied by default for operations with side effects. Explicit confirmation is required for shell writes, file writes, Git mutations, network requests, external service calls, and credentialed operations. A trace imported from another machine never grants replay permission.

## Encryption

Encrypted local storage is a planned optional layer. It is not claimed as implemented until key management, rotation, recovery, and cross-platform secure-key storage are designed and tested. Filesystem-level encryption remains compatible with AgentTrace today.

## Reporting vulnerabilities

Do not open a public issue for a vulnerability that could expose credentials, source, or trace data. Follow `SECURITY.md` once the disclosure contact/process is published in the repository.
