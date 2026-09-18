# Contributing to AgentTrace

Thanks for helping improve AgentTrace. Contributions are welcome across the trace protocol, storage, collectors, harness adapters, replay safety, CLI/API, desktop inspector, fixtures, benchmarks, documentation, and privacy tooling.

AgentTrace has one non-negotiable rule: **never fabricate telemetry**.

## Development setup

The Rust workspace requires Rust 1.85+.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets
```

For the desktop inspector:

```bash
cd apps/desktop
npm install
npm run build
```

See `docs/testing.md` for the full validation matrix.

## Design rules

1. Every normalized claim must be `native`, `inferred`, `derived`, or `unavailable` with evidence for that provenance.
2. Missing telemetry stays unavailable; do not infer richer semantics just to make adapters look uniform.
3. Preserve raw upstream records where it is safe and useful so normalization can improve later.
4. Protocol-breaking changes require a schema version change and migration/documentation updates.
5. New harness behavior must be backed by upstream source or official documentation. Do not invent flags, event fields, hooks, or headless modes.
6. Replay stays dry-run-first and exact-allowlisted. Security boundaries must not be weakened for convenience.
7. Redaction and local-first defaults are product behavior, not optional polish.
8. Add contract fixtures/tests for adapter capability claims and negative/unavailable cases.

## Harness adapters

Document the official upstream repository, supported versions, integration mode, event source, authentication assumptions, capabilities, and known limitations. Update `docs/harnesses.md` and the adapter contract fixtures in the same PR.

## Pull requests

Keep PRs focused. Explain the telemetry/provenance impact, tests run, privacy/security implications, and any schema or compatibility changes. Performance-sensitive changes should include benchmark notes where practical.

By contributing, you agree to follow `CODE_OF_CONDUCT.md` and the Apache-2.0 license terms.