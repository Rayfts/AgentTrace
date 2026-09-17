# Changelog

All notable changes to AgentTrace will be documented here.

The project is pre-1.0. Until the first stable release, protocol and CLI compatibility may still change between minor versions, but changes should remain explicit and migration-safe where persisted data is involved.

## Unreleased

### Added

- Versioned normalized event protocol with explicit native, inferred, derived, and unavailable provenance.
- SQLite + compressed raw-source local storage and interrupted-run recovery.
- Secret redaction and deny-by-default environment capture policy.
- Process supervision and normalized collection primitives.
- Built-in adapters for OpenAI Codex, Claude Code, OpenCode, Pi, Gemini CLI, Aider, Goose, Cline, Roo Code, and Continue.
- Sanitized fixture coverage and cross-adapter contract tests, including negative capability assertions.
- Adapter registry and capability evidence reporting.
- `agenttrace` CLI for harness discovery, recording, import, inspection, sanitized export, deterministic comparison, local API serving, and allowlisted replay.
- Export-time re-redaction with raw-source omission by default and explicit `--raw` opt-in.
- Dry-run-first replay in detached Git worktrees with environment scrubbing and per-command timeouts.
- Loopback-first local API through `agenttrace serve` and the standalone `agenttrace-server` binary.
- Tauri v2 + React desktop trace inspector.
- Large-trace benchmark suite for ingestion, loading, JSONL export, Rust-heap deltas, and adapter normalization, plus a storage microbenchmark.
- Cross-platform Rust CI, desktop web build validation, Tauri Windows compile validation, and dependency audit workflow.
- Tagged cross-platform binary release workflow with SHA-256 checksum files.
- Architecture, trace-format, adapter-authoring, harness, security, replay, CLI, desktop, and upstream-research documentation.

### Changed

- Aider analytics import now maps upstream `prompt_tokens` / `completion_tokens` into typed usage fields and preserves per-message provider-reported USD cost when available.
- CLI and HTTP sharing/read surfaces re-apply the current redaction policy before returning exported event data.
