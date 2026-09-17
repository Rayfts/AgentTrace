import { useCallback, useEffect, useMemo, useState } from "react";
import { databaseLocation, listRuns, loadEvents, loadStats } from "./lib/agenttrace";
import type { EventEnvelope, RunStats, RunSummary } from "./types";

type Category = "all" | "model" | "tool" | "shell" | "file" | "mcp" | "subagent" | "context" | "error";
type InspectorTab = "payload" | "raw" | "execution";

const categories: Category[] = ["all", "model", "tool", "shell", "file", "mcp", "subagent", "context", "error"];

function eventCategory(kind: string): Category {
  if (kind.startsWith("model.") || kind.startsWith("reasoning.")) return "model";
  if (kind.startsWith("tool.")) return "tool";
  if (kind.startsWith("shell.") || kind.startsWith("process.")) return "shell";
  if (kind.startsWith("file.") || kind.startsWith("git.")) return "file";
  if (kind.startsWith("mcp.")) return "mcp";
  if (kind.startsWith("subagent.")) return "subagent";
  if (kind.startsWith("context.")) return "context";
  if (kind === "error" || kind === "run.failed" || kind === "retry") return "error";
  return "all";
}

function fmtTime(value: string) {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(new Date(value));
}

function fmtDuration(ns?: number) {
  if (!ns) return "—";
  const ms = ns / 1_000_000;
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 1000) return `${ms.toFixed(0)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

function compactId(id: string) {
  return `${id.slice(0, 8)}…${id.slice(-4)}`;
}

function payloadPreview(event: EventEnvelope) {
  if (event.error?.message) return event.error.message;
  if (event.command) return [event.command.program, ...event.command.args].join(" ");
  if (event.model?.id) return event.model.id;
  const data = event.payload;
  if (typeof data === "string") return data;
  try {
    const encoded = JSON.stringify(data);
    return encoded.length > 150 ? `${encoded.slice(0, 147)}…` : encoded;
  } catch {
    return "";
  }
}

function aggregateTokens(stats?: RunStats) {
  if (!stats) return 0;
  return stats.tokens.input + stats.tokens.output + stats.tokens.cached_input + stats.tokens.reasoning_output;
}

function aggregateCost(stats?: RunStats) {
  if (!stats) return "—";
  const entries = Object.entries(stats.reported_or_deterministic_cost_by_currency);
  if (!entries.length) return "unavailable";
  return entries.map(([currency, value]) => `${currency} ${value.toFixed(4)}`).join(" · ");
}

export default function App() {
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string>();
  const [events, setEvents] = useState<EventEnvelope[]>([]);
  const [stats, setStats] = useState<RunStats>();
  const [selectedEventId, setSelectedEventId] = useState<string>();
  const [category, setCategory] = useState<Category>("all");
  const [query, setQuery] = useState("");
  const [runQuery, setRunQuery] = useState("");
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("payload");
  const [rawVisible, setRawVisible] = useState(true);
  const [dbPath, setDbPath] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [bookmarks, setBookmarks] = useState<Set<string>>(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem("agenttrace.bookmarks") ?? "[]") as string[]);
    } catch {
      return new Set();
    }
  });

  const refreshRuns = useCallback(async () => {
    try {
      const next = await listRuns();
      setRuns(next);
      if (!selectedRunId && next[0]) setSelectedRunId(next[0].run_id);
      setError("");
    } catch (cause) {
      setError(String(cause));
    }
  }, [selectedRunId]);

  useEffect(() => {
    void refreshRuns();
    void databaseLocation().then(setDbPath).catch(() => undefined);
  }, [refreshRuns]);

  useEffect(() => {
    if (!runs.some((run) => run.status === "running")) return;
    const timer = window.setInterval(() => void refreshRuns(), 2000);
    return () => window.clearInterval(timer);
  }, [runs, refreshRuns]);

  useEffect(() => {
    if (!selectedRunId) {
      setEvents([]);
      setStats(undefined);
      return;
    }
    let active = true;
    setLoading(true);
    Promise.all([loadEvents(selectedRunId, rawVisible), loadStats(selectedRunId)])
      .then(([nextEvents, nextStats]) => {
        if (!active) return;
        setEvents(nextEvents);
        setStats(nextStats);
        setSelectedEventId((current) => current && nextEvents.some((event) => event.event_id === current) ? current : nextEvents[0]?.event_id);
        setError("");
      })
      .catch((cause) => active && setError(String(cause)))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [selectedRunId, rawVisible]);

  const selectedRun = runs.find((run) => run.run_id === selectedRunId);
  const selectedEvent = events.find((event) => event.event_id === selectedEventId);

  const visibleRuns = useMemo(() => {
    const normalized = runQuery.trim().toLowerCase();
    const sorted = [...runs].sort((a, b) => Number(bookmarks.has(b.run_id)) - Number(bookmarks.has(a.run_id)));
    if (!normalized) return sorted;
    return sorted.filter((run) => `${run.harness} ${run.status} ${run.run_id}`.toLowerCase().includes(normalized));
  }, [runs, runQuery, bookmarks]);

  const visibleEvents = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return events.filter((event) => {
      const categoryMatch = category === "all" || eventCategory(event.kind) === category;
      if (!categoryMatch) return false;
      if (!normalized) return true;
      return `${event.kind} ${event.provenance.level} ${payloadPreview(event)}`.toLowerCase().includes(normalized);
    });
  }, [events, category, query]);

  const toggleBookmark = (runId: string) => {
    setBookmarks((current) => {
      const next = new Set(current);
      if (next.has(runId)) next.delete(runId); else next.add(runId);
      localStorage.setItem("agenttrace.bookmarks", JSON.stringify([...next]));
      return next;
    });
  };

  return (
    <div className="app-shell">
      <aside className="run-sidebar">
        <div className="brand-row">
          <div className="brand-mark">AT</div>
          <div><strong>AgentTrace</strong><span>local observability</span></div>
        </div>
        <div className="sidebar-tools">
          <input value={runQuery} onChange={(event) => setRunQuery(event.target.value)} placeholder="Filter runs…" aria-label="Filter runs" />
          <button className="icon-button" onClick={() => void refreshRuns()} title="Refresh runs">↻</button>
        </div>
        <div className="run-list">
          {visibleRuns.map((run) => (
            <button key={run.run_id} className={`run-row ${run.run_id === selectedRunId ? "selected" : ""}`} onClick={() => setSelectedRunId(run.run_id)}>
              <span className={`status-dot ${run.status}`} />
              <span className="run-copy"><strong>{run.harness}</strong><small>{compactId(run.run_id)} · {fmtTime(run.started_at)}</small></span>
              <span className="run-actions" onClick={(event) => { event.stopPropagation(); toggleBookmark(run.run_id); }}>{bookmarks.has(run.run_id) ? "★" : "☆"}</span>
            </button>
          ))}
          {!visibleRuns.length && <div className="empty-sidebar">No traces found.</div>}
        </div>
        <div className="db-location" title={dbPath}><span>SQLite</span><code>{dbPath || "resolving…"}</code></div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <div className="eyebrow">{selectedRun ? `${selectedRun.harness} / ${selectedRun.integration_mode}` : "TRACE INSPECTOR"}</div>
            <h1>{selectedRun ? compactId(selectedRun.run_id) : "No run selected"}</h1>
          </div>
          <div className="top-actions">
            <label className="toggle"><input type="checkbox" checked={rawVisible} onChange={(event) => setRawVisible(event.target.checked)} /><span>Raw source</span></label>
            <span className="local-badge">● local only</span>
          </div>
        </header>

        {error && <div className="error-banner">{error}</div>}

        {!selectedRun ? (
          <section className="empty-state">
            <div className="empty-glyph">⌁</div>
            <h2>Record your first agent trace</h2>
            <p>AgentTrace never invents missing telemetry. Run a supported harness and inspect the evidence it actually exposes.</p>
            <code>agenttrace run --harness codex -- codex exec "fix the failing test"</code>
          </section>
        ) : (
          <>
            <section className="metric-grid">
              <Metric label="Status" value={selectedRun.status} mono />
              <Metric label="Events" value={String(stats?.event_count ?? selectedRun.event_count)} />
              <Metric label="Tokens observed" value={aggregateTokens(stats).toLocaleString()} />
              <Metric label="Cost reported" value={aggregateCost(stats)} />
              <Metric label="Retries" value={String(stats?.event_kinds?.retry ?? 0)} />
              <Metric label="Errors" value={String((stats?.event_kinds?.error ?? 0) + (stats?.event_kinds?.["run.failed"] ?? 0))} />
            </section>

            <section className="trace-panel">
              <div className="trace-toolbar">
                <div className="category-tabs">
                  {categories.map((item) => <button key={item} className={item === category ? "active" : ""} onClick={() => setCategory(item)}>{item}</button>)}
                </div>
                <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search events…" aria-label="Search events" />
              </div>

              <div className="trace-layout">
                <div className="timeline-wrap">
                  <div className="timeline-head"><span>#</span><span>time</span><span>event</span><span>evidence</span><span>details</span><span>duration</span></div>
                  <div className="timeline-body">
                    {visibleEvents.map((event) => (
                      <button key={event.event_id} className={`event-row ${event.event_id === selectedEventId ? "selected" : ""}`} onClick={() => setSelectedEventId(event.event_id)}>
                        <span className="sequence">{event.sequence}</span>
                        <span className="event-time">{fmtTime(event.timestamp)}</span>
                        <span className={`event-kind category-${eventCategory(event.kind)}`}><i />{event.kind}</span>
                        <span><span className={`provenance ${event.provenance.level}`}>{event.provenance.level}</span></span>
                        <span className="preview">{payloadPreview(event)}</span>
                        <span className="duration">{fmtDuration(event.duration_ns)}</span>
                      </button>
                    ))}
                    {!loading && !visibleEvents.length && <div className="empty-events">No events match this filter.</div>}
                    {loading && <div className="empty-events">Loading trace…</div>}
                  </div>
                </div>

                <aside className="inspector">
                  <div className="inspector-tabs">
                    {(["payload", "raw", "execution"] as InspectorTab[]).map((tab) => <button key={tab} className={inspectorTab === tab ? "active" : ""} onClick={() => setInspectorTab(tab)}>{tab}</button>)}
                  </div>
                  {selectedEvent ? <EventInspector event={selectedEvent} tab={inspectorTab} /> : <div className="empty-inspector">Select an event.</div>}
                </aside>
              </div>
            </section>
          </>
        )}
      </main>
    </div>
  );
}

function Metric({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="metric"><span>{label}</span><strong className={mono ? "mono" : ""}>{value}</strong></div>;
}

function EventInspector({ event, tab }: { event: EventEnvelope; tab: InspectorTab }) {
  if (tab === "raw") return <JsonBlock value={event.raw_source ?? { unavailable: true, reason: "raw source not exposed or hidden" }} />;
  if (tab === "execution") {
    return <div className="inspector-content">
      <InspectorField label="Event ID" value={event.event_id} />
      <InspectorField label="Source" value={event.provenance.source} />
      <InspectorField label="Model" value={event.model ? `${event.model.provider ? `${event.model.provider}/` : ""}${event.model.id}` : "unavailable"} />
      <InspectorField label="Duration" value={fmtDuration(event.duration_ns)} />
      {event.command && <><h3>Command</h3><JsonBlock value={event.command} /></>}
      {event.filesystem_impact && <><h3>Filesystem impact</h3><JsonBlock value={event.filesystem_impact} /></>}
      {event.usage && <><h3>Usage</h3><JsonBlock value={event.usage} /></>}
      {event.cost && <><h3>Cost</h3><JsonBlock value={event.cost} /></>}
      {event.error && <><h3>Error</h3><JsonBlock value={event.error} /></>}
    </div>;
  }
  return <div className="inspector-content">
    <div className="event-title"><span className={`provenance ${event.provenance.level}`}>{event.provenance.level}</span><strong>{event.kind}</strong></div>
    {event.provenance.notes && <p className="evidence-note">{event.provenance.notes}</p>}
    <JsonBlock value={event.payload} />
  </div>;
}

function InspectorField({ label, value }: { label: string; value: string }) {
  return <div className="inspector-field"><span>{label}</span><code>{value}</code></div>;
}

function JsonBlock({ value }: { value: unknown }) {
  return <pre className="json-block">{JSON.stringify(value, null, 2)}</pre>;
}
