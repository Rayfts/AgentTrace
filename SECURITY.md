# Security Policy

AgentTrace records command output, file activity, model/tool metadata, and raw harness records when available. That makes secret handling and local execution boundaries part of the product's security model, not optional polish.

## Reporting a vulnerability

Do not publish exploit details, credentials, or sensitive trace data in a public issue.

Use GitHub's private vulnerability reporting or Security Advisory flow for this repository when it is available. If private reporting is unavailable, open a public issue containing only a request for a private maintainer contact and enough non-sensitive context to route the report. Do not include reproduction secrets or exploit payloads there.

Please include the affected AgentTrace version or commit, operating system, relevant harness, attack preconditions, and a minimal reproduction that does not contain real credentials or proprietary source code.

## Security boundaries

AgentTrace redacts known secret patterns and sensitive JSON fields before normal persistence paths. Environment capture is deny-by-default. Raw records can still contain unexpected sensitive material, so they should be treated as local sensitive data.

Replay is dry-run-first and requires an explicit allowlist. Replay uses a detached Git worktree and a scrubbed environment, but it is **not an operating-system sandbox**. An allowlisted command still executes with the current user's OS permissions and can access resources outside the worktree if the command itself does so.

The local API binds to loopback by default. Binding to a non-loopback address requires an explicit opt-in and should only be done behind an appropriate trusted network boundary.

## Supported versions

AgentTrace is pre-1.0. Security fixes are applied to the current development line; older snapshots are not guaranteed to receive backports until a stable support policy is published.
