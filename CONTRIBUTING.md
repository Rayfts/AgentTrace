# Contributing to AgentTrace

AgentTrace is an evidence-first observability project. Contributions are welcome, but telemetry claims must be grounded in something the supported harness actually exposes.

## Development setup

The core workspace requires Rust 1.85 or newer. The desktop Tauri shell is intentionally a separate workspace and currently requires Rust 1.90 or newer plus Node.js 24.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets
```

For the desktop web layer:

```bash
cd apps/desktop
npm install
npm run build
```

For the Tauri shell:

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## Adapter rules

Every adapter change must preserve the telemetry truth model:

- `native` means the value came directly from the harness or a documented integration surface.
- `inferred` means AgentTrace observed a side effect and documents the inference rule.
- `derived` means AgentTrace deterministically calculated the value from recorded evidence.
- `unavailable` means the harness does not expose the information reliably enough to claim it.

Do not fill gaps by guessing. Do not label inferred data as native. Do not depend on private or undocumented APIs when a stable public mechanism exists.

When adding or changing a harness integration, include an upstream reference in `docs/research/reference-adapters.md` or `docs/harnesses.md`, plus a small sanitized fixture whenever the upstream format can be represented safely.

## Fixtures

Fixtures must not contain real API keys, user prompts, customer repositories, private file paths, credentials, or proprietary source code. Prefer minimal synthetic records that exercise one protocol behavior at a time.

## Security-sensitive changes

Changes to redaction, replay, process execution, raw-source retention, path handling, or remote binding deserve explicit tests for the failure mode being changed. Replay must remain dry-run-first and must never silently broaden its execution allowlist.

## Pull requests

Keep commits reviewable and explain the evidence behind any new telemetry capability. PRs should state which harnesses and operating systems were tested and whether a capability is native, inferred, derived, or unavailable.
