# Harness research and capability matrix

Research snapshot: 2026-09-17. Upstream source research determines which integration modes AgentTrace is willing to implement; the matrix below reports what the **current adapters normalize today**. `agenttrace capabilities <harness>` is the runtime source of truth.

Legend: **N** native, **I** inferred, **D** derived, **—** unavailable/not currently normalized by the selected adapter.

| Harness | Implemented integration | Tools | Shell | Files/patches | MCP | Subagents | Usage | Cost | Raw structured events |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| OpenAI Codex | `codex exec --json` JSONL + JSONL import | N | N | N | N | — | N | — | N |
| Claude Code | `claude --output-format stream-json --verbose` + import; hook mode declared separately | N | N | N | N | N | N | N when reported | N |
| OpenCode | `opencode run --format json` + JSONL import | N | N | N | N | N | N | N when emitted | N |
| Pi | `pi --mode json` + JSONL import; RPC declared separately | N | N | N | — | — | N | N when reported | N |
| Gemini CLI | `--output-format stream-json` + JSONL import | N | N | N | — | — | N | — | N |
| Aider | process wrapping + `--analytics-log` JSONL import | — | — | — | — | — | N from analytics | N when reported | N for analytics records |
| Goose | `goose run --output-format stream-json` | N | — | — | N | — | N when emitted | N when reported | N |
| Cline | `cline --json` NDJSON | N | N | N | — | N | N | N when exposed | N |
| Roo Code | persisted task/API-message JSON import | N | — | — | — | — | — | — | N |
| Continue | `cn -p --format json` process capture | — | — | — | — | — | — | — | N for final/status JSON |

A blank capability is intentional. AgentTrace does not promote a generic tool record into `shell.command`, `file.write`, `mcp.request`, or another more specific event unless the adapter has enough upstream evidence to classify it safely.

## OpenAI Codex

Official repository: <https://github.com/openai/codex>

The Rust `codex exec` implementation exposes a JSONL event stream. Upstream event types include thread/turn lifecycle, item lifecycle, command execution, file changes, MCP calls, reasoning/assistant records, errors, and turn usage. Upstream Codex also has collaboration/subagent event concepts, but the current AgentTrace normalizer does not yet claim `subagent.*` support; unrecognized records are preserved as raw checkpoints instead.

Adapter strategy: direct process wrapping of an explicit `codex exec ...` invocation. The adapter injects `--json` only into that documented command shape; it refuses to silently convert the interactive TUI into headless execution. Existing JSONL can also be imported.

## Claude Code

Official repository: <https://github.com/anthropics/claude-code>

Claude Code documents hooks such as `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStop`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, and compaction-related events, and current CLI versions expose structured stream output for non-interactive execution.

Adapter strategy: the current reference implementation normalizes `--output-format stream-json --verbose` for launched/imported runs and preserves raw records. It maps exposed tool lifecycle, Bash/file operations, subagents, model usage, latency, and reported cost. A hook integration mode is declared for the stronger interactive path, but the current stream adapter does not claim a complete approval lifecycle without that hook collector.

## OpenCode

Official repository: <https://github.com/anomalyco/opencode>

`opencode run` provides a JSON format and the upstream event system includes message parts, tools, reasoning, session state/errors, permissions, and child sessions.

Adapter strategy: launch `opencode run --format json` or import its JSONL output. Current normalization covers tools, shell/file activity, MCP identity when explicit, subagents, usage, reported cost, and raw events. The non-interactive compact stream is not treated as a complete approval audit trail.

## Pi

Official repository: <https://github.com/earendil-works/pi>

Pi documents interactive, JSON, RPC, and SDK modes. `pi --mode json` emits JSON lines covering session/turn/message/tool lifecycle, provider usage, and context-compaction related records; sessions are JSONL.

Adapter strategy: launched runs use JSON mode and existing JSONL sessions can be imported. RPC is represented as a supported integration mode but is not used to invent fields missing from the JSON stream. The current normalizer covers model/tool, shell/file, context, usage, cost when reported, and raw events; MCP/subagent-specific normalization is not claimed yet.

## Gemini CLI

Official repository: <https://github.com/google-gemini/gemini-cli>

Gemini CLI defines `stream-json` records including initialization, messages, tool use/results, errors, result statistics, and per-model token accounting. It also has configurable telemetry/OTLP facilities.

Adapter strategy: AgentTrace launches or imports `stream-json`. Current normalization covers model/tool events, recognized shell/file tool activity, failures, duration, usage, and raw records. AgentTrace does not silently redirect Gemini's telemetry endpoint and does not claim cost or MCP-specific normalization from this adapter.

## Aider

Official repository: <https://github.com/Aider-AI/aider>

Aider provides one-shot message modes, local history files, Git integration, and local analytics logging. The analytics log deliberately avoids prompts/code while retaining aggregate execution/model metadata. Current upstream sample analytics records expose model identity, `prompt_tokens`, `completion_tokens`, `total_tokens`, per-message `cost`, and cumulative `total_cost` when the provider reports them.

Adapter strategy: the implemented adapter has two honest surfaces: native process supervision for stdout/stderr/failure/duration and import of explicitly supplied Aider analytics JSONL. The analytics normalizer maps model identity, prompt/completion token counts, and per-message reported USD cost into typed AgentTrace fields while preserving the raw record. It does **not** currently claim Git-diff reconstruction, file events, shell-command classification, tool calls, MCP, or subagents. Those remain future work until implemented with contract tests.

## Goose

Official repository: <https://github.com/aaif-goose/goose> (the former `block/goose` location redirects here).

Goose documents `goose run --output-format stream-json`, session management, extensions, and MCP-oriented tooling. Current stream records expose messages, tool requests/results, confirmation/action-required records, usage, errors, and completion/cost metadata when present.

Adapter strategy: launched runs require `goose run ... --output-format stream-json`. The adapter normalizes generic tool lifecycle, MCP activity, approvals, usage, reported cost, failures, and raw records. It deliberately does not relabel arbitrary Goose tools as shell/file events without a verified tool identity mapping.

## Cline

Official repository: <https://github.com/cline/cline>

The current CLI supports NDJSON through `cline --json`. Its canonical agent-event shape includes iteration lifecycle, content/tool lifecycle, usage/cost, notices/recovery, completion/errors, and team/subagent activity.

Adapter strategy: direct `cline --json` wrapping. Current normalization covers recognized shell/file tools, retries, subagents, usage/cost, final output, and raw records. AgentTrace never forces Cline's permissive approval mode merely to make automation easier, and this adapter does not currently claim a dedicated MCP event classification.

## Roo Code

Official repository: <https://github.com/RooCodeInc/Roo-Code>

The upstream repository is archived and its README states the Roo Code Extension was shut down on May 15, 2026. Its persisted task storage includes API conversation history and UI-message JSON.

Adapter strategy: import persisted task history; direct execution is intentionally unsupported. Current normalization preserves model/reasoning/tool records and context-related history where present. Shell, file, MCP, subagent, usage, cost, approval, latency, and duration fields are not promoted unless the persisted format supplies enough data and a future normalizer explicitly implements them.

## Continue

Official repository: <https://github.com/continuedev/continue>

Continue has a headless `cn -p` workflow and current versions can emit JSON final/status output. The CLI surface has changed over time, so AgentTrace avoids treating historical flags as universally available.

Adapter strategy: the current adapter accepts only an explicit headless `cn -p ...` command and requires/sets `--format json`. It treats that output as final/status data plus explicit compaction status—not as a full internal event stream. Tool calls, shell/file activity, usage, cost, approvals, MCP, and subagents remain unavailable rather than being reconstructed from decorative terminal text.

## Detection and version drift

The shared detector currently checks whether the expected binary is on `PATH` and records its `--version` output. Each adapter's start path then validates the documented command shape it knows how to normalize (for example, `codex exec`, `goose run`, or `cn -p`) and rejects incompatible explicit output modes. AgentTrace does not currently claim exhaustive `--help` capability negotiation for every installed version.

Because upstream CLIs evolve, capability reports and contract fixtures must be updated whenever an adapter changes its accepted wire format. Unsupported records should be retained as raw checkpoints rather than guessed into a richer event kind.

## Fixture policy

Adapters with structured import paths use sanitized fixture records modeled on verified public wire shapes. Fixtures contain no real credentials, private home paths, repository secrets, or user prompts. Contract tests cover positive normalization and negative capabilities: absence is a feature when upstream or the implemented adapter does not expose a field.
