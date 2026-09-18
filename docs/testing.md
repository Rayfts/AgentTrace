# Local testing and validation

This guide validates AgentTrace from a fresh checkout without requiring any hosted AgentTrace service. The Windows path is the primary desktop path; the Rust core commands are cross-platform.

## 1. Prerequisites

### All platforms

Install:

- Git
- Rust via `rustup`
- Rust components `rustfmt` and `clippy`
- Node.js 24 and npm for the desktop frontend

The core workspace declares Rust 1.85 as its MSRV. The independent Tauri desktop crate currently declares Rust 1.90. Using the current stable Rust toolchain is the simplest way to validate the whole repository.

```bash
rustup update stable
rustup default stable
rustup component add rustfmt clippy
rustc --version
cargo --version
node --version
npm --version
```

### Windows desktop prerequisites

For the Tauri desktop application, install Visual Studio 2022 Build Tools with **Desktop development with C++**. Windows 10/11 normally already has the Microsoft Edge WebView2 Runtime; install/update WebView2 if Tauri reports it missing.

### macOS desktop prerequisites

Install Xcode Command Line Tools:

```bash
xcode-select --install
```

### Linux desktop prerequisites

Install the native packages required by Tauri/WebKitGTK for your distribution before running the desktop shell. The exact package names vary by distribution; the Rust core does not require the desktop WebKit dependencies.

## 2. Clone the development branch

```bash
git clone https://github.com/Rayfts/AgentTrace.git
cd AgentTrace
git fetch origin feat/production-foundation
git switch feat/production-foundation
git status
git rev-parse HEAD
```

Keep the final SHA printed by `git rev-parse HEAD` with your test results.

## 3. Generate dependency lockfiles locally

The repository can resolve dependencies from the manifests directly. For a reproducible local test snapshot, generate the lockfiles before testing:

```bash
cargo generate-lockfile
cd apps/desktop
npm install --no-audit --no-fund
cd ../..
```

`npm install` creates `apps/desktop/package-lock.json` locally. Do not treat a dependency-resolution failure as an AgentTrace test failure until registry/network access has been ruled out.

## 4. Run the complete Rust quality gate

From the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets
cargo build --workspace
```

Expected result: every command exits with code `0`.

To check the declared core MSRV separately:

```bash
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 check --workspace --all-targets
```

If this fails only because a newly resolved third-party dependency has raised its own MSRV, capture the package/error before changing AgentTrace's declared MSRV or dependency constraints.

## 5. Build the release CLI and server

```bash
cargo build --release -p agenttrace-cli -p agenttrace-server
```

Expected binaries:

- Windows: `target/release/agenttrace.exe` and `target/release/agenttrace-server.exe`
- Linux/macOS: `target/release/agenttrace` and `target/release/agenttrace-server`

You can use `cargo run -q -p agenttrace-cli -- ...` in the remaining examples without installing the binaries globally.

## 6. Verify adapter registration and capability truthfulness

```bash
cargo run -q -p agenttrace-cli -- harnesses
cargo run -q -p agenttrace-cli -- capabilities
cargo run -q -p agenttrace-cli -- capabilities codex
cargo run -q -p agenttrace-cli -- capabilities claude-code
cargo run -q -p agenttrace-cli -- doctor
```

Check that:

- exactly ten built-in harnesses are registered;
- missing local CLIs are reported as not installed rather than as failures;
- capability output distinguishes `native`, `inferred`, `derived`, and `unavailable`;
- unsupported researched modes such as Claude hooks, Pi RPC, Cline SDK, and Roo filesystem watching are not advertised as implemented product modes.

## 7. Create an isolated fixture database

### PowerShell

```powershell
New-Item -ItemType Directory -Force .agenttrace-test | Out-Null
$db = Join-Path (Resolve-Path .agenttrace-test) 'agenttrace.db'
```

### Bash/zsh

```bash
mkdir -p .agenttrace-test
DB="$PWD/.agenttrace-test/agenttrace.db"
```

The fixture database can be deleted after testing.

## 8. Import a Codex fixture and inspect it

### PowerShell

```powershell
$import = cargo run -q -p agenttrace-cli -- --db $db import --harness codex fixtures/codex/exec.jsonl | ConvertFrom-Json
$run = $import.run_id
$run
cargo run -q -p agenttrace-cli -- --db $db inspect $run
cargo run -q -p agenttrace-cli -- --db $db inspect $run --raw
cargo run -q -p agenttrace-cli -- --db $db export $run --output .agenttrace-test/codex-export.jsonl
```

### Bash/zsh

```bash
cargo run -q -p agenttrace-cli -- --db "$DB" import --harness codex fixtures/codex/exec.jsonl
# Copy the returned run_id into RUN, then:
RUN="<run-id>"
cargo run -q -p agenttrace-cli -- --db "$DB" inspect "$RUN"
cargo run -q -p agenttrace-cli -- --db "$DB" inspect "$RUN" --raw
cargo run -q -p agenttrace-cli -- --db "$DB" export "$RUN" --output .agenttrace-test/codex-export.jsonl
```

Check that the event sequence is ordered, provenance is present, raw source is hidden by default, `--raw` reveals only retained/redacted raw evidence, and the export is newline-delimited JSON.

## 9. Verify typed latency with the Claude fixture

The sanitized Claude fixture includes native API-duration evidence used by the product registry to materialize typed `latency_ns`.

### PowerShell

```powershell
$claudeImport = cargo run -q -p agenttrace-cli -- --db $db import --harness claude-code fixtures/claude-code/stream.jsonl | ConvertFrom-Json
$claudeRun = $claudeImport.run_id
cargo run -q -p agenttrace-cli -- --db $db inspect $claudeRun --raw
```

### Bash/zsh

```bash
cargo run -q -p agenttrace-cli -- --db "$DB" import --harness claude-code fixtures/claude-code/stream.jsonl
```

Inspect the terminal `run.completed` event and verify that explicit Claude `duration_api_ms` evidence is represented as typed `latency_ns`. Other harnesses must not receive latency merely because they have a duration field.

## 10. Verify deterministic run comparison

Import the Codex fixture a second time, then compare the two run IDs:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> import --harness codex fixtures/codex/exec.jsonl
cargo run -q -p agenttrace-cli -- --db <db-path> compare <first-run-id> <second-run-id>
```

Comparison should contain deterministic event/provenance/usage/cost measurements only. It must not produce an LLM-generated winner or fabricate missing costs.

## 11. Verify replay safety

First inspect replay without execution:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> replay <run-id> --repo .
```

Expected behavior:

- mode is `dry_run`;
- only normalized `shell.command` events appear as executable candidates;
- commands include advisory risk tags where applicable;
- no historical command executes.

Now verify that execution without an exact allowlist is rejected:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> replay <run-id> --repo . --execute
```

Expected result: AgentTrace refuses execution because no exact `--allow` or `--allow-sequence` entry was supplied.

For a command you have personally reviewed, optionally test exact execution. For example, if the dry-run plan contains exactly `cargo test`:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> replay <run-id> --repo . --execute --allow "cargo test"
```

Verify that replay reports a detached-worktree execution result. Remember that replay is repository-isolated, **not** an operating-system sandbox.

## 12. Verify the local HTTP API

Start the API in terminal 1:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> serve
```

In terminal 2:

```bash
curl http://127.0.0.1:4319/api/health
curl http://127.0.0.1:4319/api/harnesses
curl "http://127.0.0.1:4319/api/capabilities?harness=codex"
curl http://127.0.0.1:4319/api/runs
curl http://127.0.0.1:4319/api/runs/<run-id>/events
curl http://127.0.0.1:4319/api/runs/<run-id>/stats
```

The health endpoint should return `ok: true`. Raw source should remain omitted unless explicitly requested with `?raw=true` on endpoints that support it.

Verify the remote-bind guard separately:

```bash
cargo run -q -p agenttrace-cli -- --db <db-path> serve --bind 0.0.0.0:4319
```

Expected result: AgentTrace refuses the non-loopback bind unless `--allow-remote` is explicitly supplied.

Stop the local API with Ctrl+C.

## 13. Build the React desktop frontend

```bash
cd apps/desktop
npm install --no-audit --no-fund
npm run build
```

Expected result: TypeScript compilation and the Vite production build both complete successfully.

## 14. Check the Tauri Rust shell

From `apps/desktop`:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

This validates the independent desktop Rust workspace, including the dialog plugin, adapter registry, storage, redaction, and Tauri command surface.

## 15. Run the desktop inspector against the fixture database

### PowerShell

From `apps/desktop`:

```powershell
$env:AGENTTRACE_DB = (Resolve-Path ..\..\.agenttrace-test\agenttrace.db).Path
npm run tauri dev
```

### Bash/zsh

```bash
export AGENTTRACE_DB="$(cd ../.. && pwd)/.agenttrace-test/agenttrace.db"
npm run tauri dev
```

Verify in the UI:

1. imported runs appear in the left run history;
2. selecting a run loads the normalized timeline;
3. search and all event-category filters work;
4. provenance badges show native/inferred/derived/unavailable truthfully;
5. raw-source toggle hides and shows retained raw evidence;
6. payload, execution, terminal, diff, and relations inspectors never fabricate absent data;
7. token/event/duration charts show only observed data;
8. the Claude fixture produces the explicit API-latency chart from `latency_ns`;
9. capability chips expose unavailable categories rather than omitting them;
10. run comparison works;
11. artifact metadata renders when artifacts exist;
12. System, Dark, and Light themes all work and persist;
13. **Export sanitized** downloads a JSONL export;
14. **Import trace** opens a native file picker and successfully imports a supported fixture when the correct harness is selected;
15. `browser` is present as a timeline category but remains empty unless a trace actually contains verified `browser.*` events.

For the desktop import test, select `Claude Code`, choose `fixtures/claude-code/stream.jsonl`, and confirm that the newly imported run appears without restarting the application.

## 16. Build the packaged desktop application

After the development inspector passes:

```bash
cd apps/desktop
npm run tauri build
```

This is the closest local equivalent to validating the distributable Tauri application. Platform bundle/signing requirements can differ from a normal development build.

## 17. Run performance benchmarks

From the repository root:

```bash
cargo run --release -p agenttrace-benchmarks
cargo run --release -p agenttrace-storage --example ingest_benchmark -- 10000
```

The large-trace benchmark deterministically expands the checked-in recipe to 100,000 schema-v2 events. Record the machine specifications with benchmark output; do not compare absolute timings across unlike hardware as if they were a product guarantee.

## 18. Optional real-harness smoke test

If OpenAI Codex is installed locally:

```bash
cargo run -q -p agenttrace-cli -- run --harness codex -- codex exec "Inspect this repository and reply with one short sentence. Do not modify files."
```

Use `agenttrace harnesses` and `docs/harnesses.md` before testing another harness. AgentTrace intentionally rejects unsupported command shapes instead of inventing headless flags.

## 19. Final pass checklist

Before treating a revision as locally validated, confirm all of the following passed on the **same commit SHA**:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo check --workspace --all-targets`
- core release build
- fixture import/inspect/export
- replay dry-run and missing-allow rejection
- local API smoke test and remote-bind rejection
- `npm run build`
- desktop Tauri `cargo check`
- `npm run tauri dev` manual inspector checks
- optional `npm run tauri build`
- benchmarks when performance changes are under review

If one command fails, keep the first complete error output and the commit SHA. Do not continue by changing multiple unrelated dependencies at once; isolate the failing surface first.
