# Changelog

All notable changes to AgentTrace will be documented here.

The project is pre-1.0. Until the first stable release, protocol and CLI compatibility may still change between minor versions, but changes should remain explicit and migration-safe where persisted data is involved.

## Unreleased

### Added

- Versioned normalized event protocol with explicit native, inferred, derived, and unavailable provenance.
- Schema v2 browser event namespace for `browser.navigation`, `browser.action`, `browser.network`, and `browser.console`, without granting adapters permission to infer browser telemetry from generic tool names.
- Typed optional `latency_ns` evidence, with Claude Code's explicit `duration_api_ms` promoted at the product registry boundary while other harnesses remain unavailable unless they expose an equally explicit signal.
- SQLite/WAL local trace storage with indexed event access, compression, migrations, and interrupted-run recovery.
- Run-scoped artifact storage with optional event linkage, SHA-256 identity, media type, original byte size, compression, and cross-run linkage validation.
- Built-in secret/path redaction plus additive JSON redaction profiles for custom regexes, sensitive JSON keys, and sensitive path fragments.
- Deny-by-default environment capture policy.
- Process supervision and normalized collection primitives.
- Built-in adapters for OpenAI Codex, Claude Code, OpenCode, Pi, Gemini CLI, Aider, Goose, Cline, Roo Code, and Continue.
- Sanitized fixture coverage and cross-adapter contract tests, including negative capability assertions.
- Product-facing adapter registry that completes all 21 adapter capability categories with explicit unavailable evidence and filters researched-but-unimplemented integration modes.
- `agenttrace` CLI for harness discovery, recording, import, inspection, sanitized export, deterministic comparison, local API serving, and allowlisted replay.
- Export-time re-redaction with raw-source omission by default and explicit `--raw` opt-in.
- `--redaction-config` support across CLI recording/import/inspection/export/serve and the standalone server.
- Dry-run-first replay in detached Git worktrees with environment scrubbing, per-command timeouts, and advisory per-command risk tags for shell execution, filesystem/Git mutation, network/external-service access, and credential-sensitive arguments.
- Loopback-first local API through `agenttrace serve` and the standalone `agenttrace-server` binary.
- Tauri v2 + React desktop trace inspector with run history, live active-run polling, native adapter-backed trace import, search/filtering including browser events, capability evidence, payload/raw/execution/terminal views, evidence-backed unified/split diffs, span/subagent/context relationships, deterministic run comparison, sanitized JSONL export, and artifact metadata.
- Desktop observed-only charts for token usage, explicit API latency, event distribution, and event duration evidence.
- Persistent desktop System/Dark/Light themes.
- Desktop support for the same additive redaction profile through `AGENTTRACE_REDACTION_CONFIG`.
- Official Tauri dialog-plugin integration scoped to user-initiated trace file selection.
- Large-trace benchmark suite for ingestion, loading, JSONL export, Rust-heap deltas, and adapter normalization, plus a storage microbenchmark; deterministic benchmark recipe updated to schema v2.
- Cross-platform Rust CI, desktop web build validation, Tauri Windows compile validation, and dependency audit workflow.
- Tagged cross-platform binary release workflow with SHA-256 checksum files.
- Architecture, trace-format, adapter-authoring, harness, security, replay, CLI, desktop, upstream-research, and complete local-testing documentation.

### Changed

- Project licensing is Apache-2.0 across the root Rust workspace, desktop Rust package, npm package metadata, README, and top-level license text.
- Aider analytics import maps upstream `prompt_tokens` / `completion_tokens` into typed usage fields and preserves per-message provider-reported USD cost when available.
- CLI, HTTP, and desktop sharing/read surfaces re-apply the active redaction policy before returning event data.
- Runtime capability output no longer advertises Claude Code hooks, Pi RPC, Cline SDK, or Roo Code filesystem-watch as implemented product modes; those remain researched follow-on surfaces until real collector/control paths exist.
- Capability consumers no longer need to infer unsupported telemetry from missing map keys because the registry materializes explicit unavailable evidence.
- Desktop run comparison keeps reported cost partitioned by currency instead of summing incompatible currencies.
- Replay exact allowlisting remains the execution gate; risk tags make likely side effects visible without broadening authorization to entire side-effect categories.
