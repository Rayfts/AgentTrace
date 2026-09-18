import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { importTrace } from "../lib/agenttrace";

const harnesses = [
  ["codex", "OpenAI Codex"],
  ["claude-code", "Claude Code"],
  ["opencode", "OpenCode"],
  ["pi", "Pi"],
  ["gemini", "Gemini CLI"],
  ["aider", "Aider"],
  ["goose", "Goose"],
  ["cline", "Cline"],
  ["roo-code", "Roo Code"],
  ["continue", "Continue"],
] as const;

type Props = {
  onImported: (runId: string, eventCount: number) => void | Promise<void>;
  onError: (message: string) => void;
};

export function ImportPanel({ onImported, onError }: Props) {
  const [harness, setHarness] = useState("codex");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  const chooseAndImport = async () => {
    try {
      const selection = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "Agent trace/session", extensions: ["json", "jsonl", "ndjson", "log"] },
        ],
      });
      if (!selection || Array.isArray(selection)) return;
      setBusy(true);
      setMessage("Importing…");
      const result = await importTrace(harness, selection);
      setMessage(`${result.imported_events.toLocaleString()} events imported`);
      await onImported(result.run_id, result.imported_events);
    } catch (cause) {
      const text = String(cause);
      setMessage("Import failed");
      onError(text);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="import-panel">
      <div className="import-row">
        <select value={harness} onChange={(event) => setHarness(event.target.value)} aria-label="Import harness">
          {harnesses.map(([value, label]) => <option value={value} key={value}>{label}</option>)}
        </select>
        <button className="secondary-button" disabled={busy} onClick={() => void chooseAndImport()}>
          {busy ? "Importing" : "Import trace"}
        </button>
      </div>
      {message && <small>{message}</small>}
    </div>
  );
}
