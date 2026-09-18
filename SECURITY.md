# Security Policy

AgentTrace records and replays developer-tool activity, so traces may contain source code, commands, paths, prompts, tool payloads, and other sensitive engineering data.

## Reporting a vulnerability

Do not publish exploit details, credentials, sensitive traces, replay bypasses, redaction bypasses, or remote-access weaknesses in a public issue.

If GitHub private vulnerability reporting is enabled, use **Security → Report a vulnerability**. Otherwise, open a minimal non-sensitive issue requesting a private reporting channel.

Include the affected commit/version, operating system, minimal reproduction, impact, and whether the issue can expose trace contents, local files, credentials, execute unapproved commands, or bypass loopback/redaction protections.

## Security boundaries

- No automatic trace upload exists; storage is local-first.
- Environment capture is deny-by-default and must not indiscriminately persist secrets.
- Redaction is applied before normal persistence and again on read/export surfaces.
- The local API binds to loopback by default; non-loopback binding requires explicit opt-in.
- Replay is dry-run-first and requires explicit execution plus an exact recorded command/sequence allowlist.
- Replay's temporary Git worktree protects repository state but is **not an operating-system sandbox**.
- Raw payloads can still contain sensitive information; users should review traces before sharing them.

Changes to redaction, replay, API binding, artifact handling, or credential scrubbing should include focused regression tests and relevant documentation updates.