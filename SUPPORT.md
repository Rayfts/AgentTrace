# Support

AgentTrace is a pre-1.0 open-source project. Community support is best-effort; there is no guaranteed response time or commercial SLA.

## Before opening an issue

Check the README and relevant documents under `docs/`, then run:

```bash
agenttrace doctor
agenttrace harnesses
```

For harness-specific problems, include the harness name/version and `agenttrace capabilities <id>` output when safe to share.

## Bugs

Use the bug-report issue form. Include the AgentTrace commit/version, OS, Rust version, harness version/integration mode, the smallest reproducible trace or fixture, and sanitized logs. Never attach credentials, private source, or an unsanitized trace unintentionally.

## Feature requests

Use the feature-request form. Describe the debugging/observability problem first. For new telemetry, explain which upstream surface exposes it and what provenance level it should have.

## Security

Do not report vulnerabilities through a normal issue. Follow `SECURITY.md`.

## Questions

Focused usage questions are welcome as GitHub issues when the README/docs do not already answer them. Keep one problem per issue so answers remain searchable.