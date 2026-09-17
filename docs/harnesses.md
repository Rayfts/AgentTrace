# Harness research and capability matrix

Research snapshot: 2026-09-17. The matrix documents the strongest **verified public integration surface** found in the upstream repositories. Runtime detection must still inspect the installed version because harness behavior changes.

Legend: **N** native, **I** inferred, **D** derived, **—** unavailable/not promised by the selected integration.

| Harness | Primary integration | Tools | Shell | Files/patches | MCP | Subagents | Usage | Cost | Raw structured events |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| OpenAI Codex | `codex exec --json` JSONL | N | N | N | N | N | N | — | N |
| Claude Code | documented hooks + transcript; stream-json when installed CLI advertises it | N | N via tool hooks | N via tool hooks | tool-dependent | N stop hook | runtime-dependent | — | N via hooks/stream |
| OpenCode | `opencode run --format json` / event subscription | N | N | N | N | N | N when emitted | N when emitted | N |
| Pi | `pi --mode json`, RPC, JSONL sessions | N | N | N | extension/tool dependent | extension dependent | N | N when provider reports | N |
| Gemini CLI | `--output-format stream-json` | N | tool-dependent | tool-dependent | N/tool-dependent | — | N | — | N |
| Aider | process + chat/LLM history + Git/worktree observation | I/N history | I process output | D/I via Git + history | — | — | history/provider dependent | history/provider dependent | — |
| Goose | `goose run --output-format stream-json` | N | N | N | N (extensions) | harness-dependent | N when emitted | provider-dependent | N |
| Cline | `cline --json` NDJSON | N | N | N | N | N/team events | N | N when exposed | N |
| Roo Code | persisted task/API message import and file watching | N history | I from persisted messages | N/I from task history | message-dependent | task-dependent | message-dependent | message-dependent | N persisted JSON |
| Continue | `cn -p` headless; JSON output only when installed version exposes `--format json` | N when JSON emits tool records | N when JSON emits tool records | N/I | N | agent-dependent | version/provider dependent | — | version-dependent |

## OpenAI Codex

Official repository: <https://github.com/openai/codex>

The Rust `codex exec` implementation has a documented `--json` option that prints JSONL. Its `ThreadEvent` includes thread/turn lifecycle, item lifecycle, and errors. Item variants include agent messages, reasoning summaries, command execution with aggregated output and exit code, file changes, MCP tool calls, collaboration/subagent calls, web search, todo lists, and errors. Turn completion includes input, cached input, cache-write input, output, and reasoning-output token usage.

Adapter strategy: direct process wrapping of `codex exec --json`, preserving each JSONL record. Runtime detection checks `codex --version` and `codex exec --help`; AgentTrace will not append flags to arbitrary `codex` invocations unless the detected command shape supports them.

## Claude Code

Official repository: <https://github.com/anthropics/claude-code>

The public repository documents hooks for `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStop`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreCompact`, and notifications. Hook stdin contains a session ID, transcript path, cwd, permission mode, event name, and event-specific tool/prompt/result fields. The upstream changelog also documents `--output-format stream-json` behavior.

Adapter strategy: prefer documented hook capture for interactive sessions because it observes approvals/tool lifecycle without scraping the terminal. For non-interactive use, enable stream JSON only after runtime help/version detection confirms the installed CLI supports it. Transcript import supplements history but does not retroactively make missing timing data native.

## OpenCode

Official repository: <https://github.com/anomalyco/opencode>

`opencode run` is explicitly non-interactive and accepts `--format json` for raw event streaming. The implementation subscribes to the event stream and handles message parts, tool completion/error, reasoning, session errors/status, permissions, and child sessions. The SDK also exposes SSE event subscriptions.

Adapter strategy: wrap `opencode run --format json` or attach to an explicitly configured OpenCode server event stream. Preserve session IDs and parent session relationships.

## Pi

Official repository: <https://github.com/earendil-works/pi>

Pi documents four modes: interactive, print/JSON, RPC, and SDK. `pi --mode json` emits JSON lines with session header plus agent/turn/message/tool lifecycle. Sessions are stored as JSONL under `~/.pi/agent/sessions/`; the JSON mode includes provider-reported usage and compaction/queue events.

Adapter strategy: JSON mode for launched runs; RPC for deeper embedded control when requested; session JSONL import for existing runs. AgentTrace treats provider usage as native to Pi's event stream and never assumes nonzero usage before the provider reports it.

## Gemini CLI

Official repository: <https://github.com/google-gemini/gemini-cli>

The core defines `stream-json` JSONL events: `init`, `message`, `tool_use`, `tool_result`, `error`, and `result`. Result stats include total/input/output/cached/input token counts, duration, tool-call count, and per-model breakdowns. Gemini CLI also exposes configurable telemetry/OTLP settings, but AgentTrace must not silently redirect a user's existing telemetry endpoint.

Adapter strategy: structured stream JSON for launched runs. Optional OTLP ingestion is an explicit advanced integration, never a default side effect.

## Aider

Official repository: <https://github.com/Aider-AI/aider>

Aider documents `--message`/`--message-file` for one-shot non-chat execution, `--chat-history-file`, and `--llm-history-file`. It also has strong Git integration. The researched public CLI does not promise a universal native JSON event stream equivalent to Codex/Pi/Gemini.

Adapter strategy: process supervision plus explicitly configured history files and before/after Git state. stdout/stderr timing is native to AgentTrace's process wrapper; changes derived from Git snapshots are marked derived/inferred. Model/tool fields that are not present in the history remain unavailable.

## Goose

Official repository: <https://github.com/aaif-goose/goose> (the former `block/goose` location redirects here).

Goose documents `goose run` with `--output-format text|json|stream-json`, session management, stdio/HTTP extensions, and debug output. Current session storage uses SQLite (`sessions.db`) from v1.10 onward, with legacy JSONL import/export support.

Adapter strategy: `stream-json` for launched runs, preserving extension/MCP events; explicit session export/import for existing runs rather than reading the live Goose database behind its back unless a compatible read-only schema is documented.

## Cline

Official repository: <https://github.com/cline/cline>

The current CLI is first-class: `cline --json` emits NDJSON. Its canonical `AgentEvent` includes iteration lifecycle, text/reasoning/tool content start/update/end, usage (including optional cache and cost fields), notices/recovery, final result, and errors. Team events expose subagent/team activity. CLI JSON mode writes the canonical agent event inside an `agent_event` envelope.

Adapter strategy: direct `cline --json` wrapping. Runtime configuration can keep normal approvals; AgentTrace must never force `--yolo` just to make automation easier.

## Roo Code

Official repository: <https://github.com/RooCodeInc/Roo-Code>

The upstream repository is archived and its README states the Roo Code Extension was shut down on May 15, 2026. The codebase nevertheless documents persisted per-task `api_conversation_history.json` and `ui_messages.json`, plus task history storage.

Adapter strategy: import/watch historical task storage only. AgentTrace will not pretend a maintained headless Roo CLI exists. Persisted records can expose messages, reasoning fields, tool records, and metadata where present; missing timing/approval data remains unavailable.

## Continue

Official repository: <https://github.com/continuedev/continue>

Continue has a headless `cn -p` mode and current repository history documents JSON output support through `--format json` in versions that expose the flag. The project has changed its CLI surface over time, including recent removal of old Hub/login behavior, so hard-coded historical flags are risky.

Adapter strategy: detect `cn --version` and parse `cn --help`/headless help before selecting JSON mode. If structured output is absent, AgentTrace falls back to process events and explicitly marks internal agent/tool telemetry unavailable rather than parsing decorative terminal text.

## Fixture policy

Each adapter owns sanitized fixtures copied from or modeled on the documented public wire shape. Fixtures contain no real credentials, home paths, repository secrets, or user prompts. Contract tests assert both normalization and **negative capabilities**: absence is a feature when the upstream surface does not expose a field.
