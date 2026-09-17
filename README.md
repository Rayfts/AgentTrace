# AgentTrace

AgentTrace is a local-first debugger, recorder, profiler, and observability platform for AI coding agents.

The goal is simple: when an agent changes a repository, developers should be able to inspect **what actually happened** instead of trusting a reconstructed story after the fact.

AgentTrace is being built primarily in Rust. The core trace protocol, collectors, normalization, storage, process supervision, redaction, replay safety, comparison, and adapters live in Rust. A Tauri + React/TypeScript desktop UI is planned as a thin visualization layer over the same local API.

## Telemetry truth model

AgentTrace never fabricates telemetry. Every normalized field is tagged by provenance:

- **native** — emitted directly by the harness or its documented integration surface;
- **inferred** — observed from a side effect with an explicit inference rule;
- **derived** — deterministically computed from native/inferred data;
- **unavailable** — not exposed reliably enough to report.

Raw source records are retained when safe so future AgentTrace versions can improve normalization without pretending older collectors observed data they never had.

## Initial harnesses

The initial adapter set targets OpenAI Codex, Claude Code, OpenCode, Pi, Gemini CLI, Aider, Goose, Cline, Roo Code, and Continue. Integration is intentionally different per harness: structured event streams are preferred where they exist; hooks, session import, storage observation, or conservative process wrapping are used elsewhere.

See [`docs/harnesses.md`](docs/harnesses.md) for the current capability matrix and upstream evidence, [`docs/trace-format.md`](docs/trace-format.md) for the normalized protocol, and [`docs/architecture.md`](docs/architecture.md) for system design.

## Planned CLI

```text
agenttrace run --harness codex -- codex exec ...
agenttrace import <path>
agenttrace inspect <run-id>
agenttrace export <run-id>
agenttrace compare <run-a> <run-b>
agenttrace doctor
agenttrace harnesses
agenttrace capabilities codex
agenttrace serve
```

A harness adapter may reject `run` when direct execution is not a legitimate integration mode. AgentTrace will not invent a headless interface for an editor-only or discontinued harness.

## Repository status

AgentTrace is under active construction. The first committed foundation includes the versioned event protocol, adapter API, architecture, and researched capability matrix. Storage, collectors, concrete adapters, CLI, server, replay, and desktop work follow on the same branch in reviewable commits.

## License

MIT. The repository started with an MIT license and the project keeps that license unless maintainers explicitly decide otherwise.
