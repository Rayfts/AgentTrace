## Summary

Describe the user-visible or architectural change.

## Evidence / telemetry impact

- Harnesses affected:
- New or changed capability claims:
- Provenance level (`native`, `inferred`, `derived`, `unavailable`):
- Upstream documentation / fixture evidence:

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] `cargo check --workspace --all-targets`
- [ ] Desktop web build checked when UI code changed
- [ ] Tauri shell checked when desktop Rust code changed

## Security / privacy

Describe any change to raw-source retention, redaction, environment capture, process execution, replay, file paths, networking, or remote binding. Write `none` when not applicable.
