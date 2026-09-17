# Replay safety

AgentTrace replay exists to re-run recorded shell evidence deliberately, not to reanimate an entire autonomous agent session.

## What can be replayed

Only normalized `shell.command` events with structured command metadata are eligible. Model requests, model responses, tool calls, MCP calls, approvals, subagents, reasoning events, file writes, and inferred events are never automatically executed by replay.

## Dry run first

`agenttrace replay <run-id> --repo <repo>` only builds and prints a replay plan. Execution requires `--execute` plus at least one allowlist entry.

An allowlist entry can identify the exact recorded command display string:

```bash
--allow "cargo test"
```

or the exact normalized event sequence number:

```bash
--allow-sequence 42
```

Unlisted commands remain blocked. If an allowlisted command fails, later allowlisted commands are skipped unless `--continue-on-error` is present.

## Repository isolation

Execution takes place in a temporary detached Git worktree at the requested revision. This prevents normal replayed file changes from modifying the caller's checked-out worktree directly. AgentTrace removes the temporary worktree after the replay attempt.

This is repository-state isolation, not a security sandbox.

## OS permissions

Replay commands still execute as the current operating-system user. A command can deliberately reference absolute paths, network resources, sockets, processes, or other files outside the detached worktree if the underlying operating system allows it.

AgentTrace therefore does **not** describe replay as an OS sandbox and does not claim it can safely execute untrusted commands.

## Environment

Replay clears the inherited process environment and selectively restores basic execution variables such as `PATH`, platform command-resolution variables, temporary-directory variables, and locale settings. API keys and arbitrary environment variables are not intentionally propagated.

A command can still obtain credentials through other operating-system mechanisms if those credentials are available to the current user. Environment scrubbing is one defense layer, not complete isolation.

## Timeouts

Every replayed command has a per-command timeout. The default is 300 seconds and can be changed with `--timeout-seconds`. Timed-out commands are reported distinctly from ordinary non-zero exits.

## Recommended use

Use replay for deterministic developer commands you recognize: tests, linters, builds, formatters, or reproduction commands. Review the dry-run plan before execution. Do not allowlist a command merely because it appeared in a historical trace.
