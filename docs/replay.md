# Replay safety

AgentTrace replay exists to re-run recorded shell evidence deliberately, not to reanimate an entire autonomous agent session.

## What can be replayed

Only normalized `shell.command` events with structured command metadata are eligible. Model requests, model responses, tool calls, browser events, MCP calls, approvals, subagents, reasoning events, file events, and inferred events are never automatically executed by replay.

## Dry run first

`agenttrace replay <run-id> --repo <repo>` only builds and prints a replay plan. Execution requires `--execute` plus at least one exact allowlist entry.

Each planned command includes conservative advisory risk tags when recognizable:

- `shell_execution` — every replay command executes a program or shell;
- `filesystem_mutation` — common write/delete/move/redirection patterns;
- `git_mutation` — Git operations that may change repository or remote state;
- `network_access` — common network clients, remote Git operations, package install/publish patterns, or explicit URLs;
- `external_service` — recognizable cloud/service CLIs or service endpoints;
- `credential_sensitive` — arguments containing common authorization, token, password, API-key, or secret markers.

Risk tags are intentionally conservative and are **not** a complete shell analyzer. Their purpose is to make likely side effects visible in the dry-run plan. They do not grant execution permission and they do not turn replay into a sandbox.

## Exact confirmation

An allowlist entry can identify the exact recorded command display string:

```bash
--allow "cargo test"
```

or the exact normalized event sequence number:

```bash
--allow-sequence 42
```

The exact allowlist entry confirms that specific recorded command together with the risk tags shown in the plan. There is no broad "allow network", "allow writes", or "allow Git" switch that silently authorizes other commands.

Unlisted commands remain blocked. If an allowlisted command fails, later allowlisted commands are skipped unless `--continue-on-error` is present.

## Repository isolation

Execution takes place in a temporary detached Git worktree at the requested revision. This prevents normal replayed file changes from modifying the caller's checked-out worktree directly. AgentTrace removes the temporary worktree after the replay attempt.

This is repository-state isolation, not a security sandbox. Commands can still refer to absolute paths or mutate repositories/remotes beyond the temporary worktree if the current operating-system user has permission.

## OS permissions

Replay commands execute as the current operating-system user. A command can deliberately reference absolute paths, network resources, sockets, processes, or other files outside the detached worktree if the underlying operating system allows it.

AgentTrace therefore does **not** describe replay as an OS sandbox and does not claim it can safely execute untrusted commands.

## Environment and credentials

Replay clears the inherited process environment and selectively restores basic execution variables such as `PATH`, platform command-resolution variables, temporary-directory variables, and locale settings. API keys and arbitrary environment variables are not intentionally propagated.

A command can still obtain credentials through other operating-system mechanisms if those credentials are available to the current user—for example credential helpers, local config files, SSH agents, browser sessions, or OS key stores. Environment scrubbing and the `credential_sensitive` risk tag are defense/visibility layers, not complete isolation.

## Timeouts

Every replayed command has a per-command timeout. The default is 300 seconds and can be changed with `--timeout-seconds`. Timed-out commands are reported distinctly from ordinary non-zero exits.

## Recommended use

Use replay for deterministic developer commands you recognize: tests, linters, builds, formatters, or reproduction commands. Review the dry-run plan and its risk tags before execution. Do not allowlist a command merely because it appeared in a historical trace.
