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
- Adapter registry and capability evidence reporting.
- `agenttrace` CLI for harness discovery, recording, import, inspection, export, deterministic comparison, and allowlisted replay.
- Dry-run-first replay in detached Git worktrees with environment scrubbing and per-command timeouts.
- Loopback-first `agenttrace-server` local API.
- Tauri v2 + React desktop trace inspector.
- Cross-platform Rust CI, desktop web build validation, Tauri Windows compile validation, and dependency audit workflow.
- Architecture, trace-format, harness, security, replay, CLI, and desktop documentation.
