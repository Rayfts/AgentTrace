# Desktop inspector

AgentTrace includes a Tauri v2 + React desktop inspector under `apps/desktop`.

The desktop app is a local visualization layer over the same SQLite trace format used by the CLI. It does not require a hosted backend.

## Features

- run browser with status, harness, timestamp, and local bookmarks;
- live refresh while a run is active;
- event timeline with category filters and text search;
- explicit native / inferred / derived / unavailable provenance badges;
- payload, raw-source, and execution inspectors;
- command, filesystem-impact, token, cost, duration, retry, and error views when the trace actually contains those fields;
- raw-source visibility toggle;
- compact dark DevTools-style layout intended for high-density debugging.

## Development

Install frontend dependencies and start Tauri from `apps/desktop`:

```bash
npm install
npm run tauri dev
```

The Tauri crate is deliberately isolated from the root Rust workspace because the desktop runtime may require a newer Rust compiler than the AgentTrace core MSRV.

## Database selection

The desktop shell uses `AGENTTRACE_DB` when that environment variable is set. Otherwise it creates or opens `agenttrace.db` in the platform-specific AgentTrace application-data directory.

To inspect the same database as a CLI session, point `AGENTTRACE_DB` at the CLI database before launching the desktop app.

## Trust model

The desktop app displays stored evidence. It does not reinterpret unavailable fields as observed telemetry. Raw-source views show only what was retained after AgentTrace's redaction path.
