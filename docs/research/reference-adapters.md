# Reference adapter research notes

Research snapshot: 2026-09-17.

These notes pin the concrete integration surfaces used by the first AgentTrace adapters. They are intentionally conservative: an event is normalized only when the upstream harness actually exposes it.

## OpenAI Codex

Official repository: `openai/codex`.

The current Rust CLI exposes structured execution through `codex exec --json`. The JSON stream uses lifecycle events such as `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.started`, `item.updated`, and `item.completed`. Documented item types include `assistant_message`, `reasoning`, `command_execution`, `file_change`, `mcp_tool_call`, `web_search`, `todo_list`, and `error`. `turn.completed` includes token usage (`input_tokens`, `cached_input_tokens`, `output_tokens`).

AgentTrace therefore treats `codex exec --json` as the primary reference integration. It will not reinterpret the interactive TUI as if it emitted the same structured stream.

Evidence reviewed in the official repository includes PR #4525 (current `codex exec --json` event/item shape), PR #4177 (the event-model design), and PR #1603 (the original JSON execution mode).

## Claude Code

Official repository: `anthropics/claude-code`.

Claude Code provides non-interactive print mode and supports `stream-json` as an output format. The public repository also exposes a mature hook/plugin surface and stores session transcripts as JSONL under the Claude project data area; current community/plugin code in the official repository reads those transcripts directly for deterministic session export. Hook payloads expose tool name/input and session context, and recent hook work documents subagent context fields.

The first AgentTrace reference adapter therefore prefers Claude Code's native non-interactive structured output for launched runs, with hooks/transcript import as complementary modes. It does not claim that every interactive-TUI detail is present in the stream.

## Adapter policy

For both adapters:

- raw source records are preserved alongside normalized events when safe;
- token/cost fields are populated only when the harness reports them or a deterministic price calculation is explicitly configured;
- reasoning is recorded only when the harness exposes reasoning/thinking content;
- unknown upstream event types are retained rather than guessed into a misleading AgentTrace event;
- interactive-only behavior is not silently converted into headless execution.
