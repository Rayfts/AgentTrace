import { useCallback, useEffect, useMemo, useState } from "react";
import { ImportPanel } from "./components/ImportPanel";
import { ThemeControl } from "./components/ThemeControl";
import { TraceCharts } from "./components/TraceCharts";
import {
  compareRuns,
  databaseLocation,
  exportSanitizedRun,
  listArtifacts,
  listRuns,
  loadCapabilities,
  loadEvents,
  loadStats,
} from "./lib/agenttrace";
import type {
  ArtifactMetadata,
  CapabilityReport,
  EventEnvelope,
  RunComparison,
  RunStats,
  RunSummary,
} from "./types";

type Category = "all" | "model" | "tool" | "shell" | "file" | "browser" | "mcp" | "subagent" | "context" | "error";
type InspectorTab = "payload" | "raw" | "execution" | "terminal" | "diff" | "relations";
type DiffMode = "unified" | "split";

const categories: Category[] = ["all", "model", "tool", "shell", "file", "browser", "mcp", "subagent", "context", "error"];
const inspectorTabs: InspectorTab[] = ["payload", "raw", "execution", "terminal", "diff", "relations"];

function eventCategory(kind: string): Category {
  if (kind.startsWith("model.") || kind.startsWith("reasoning.")) return "model";
  if (kind.startsWith("tool.")) return "tool";
  if (kind.startsWith("shell.") || kind.startsWith("process.")) return "shell";
  if (kind.startsWith("file.") || kind.startsWith("git.")) return "file";
  if (kind.startsWith("browser.")) return "browser";
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
  const [capabilities, setCapabilities] = useState<CapabilityReport>();
  const [artifacts, setArtifacts] = useState<ArtifactMetadata[]>([]);
  const [selectedEventId, setSelectedEventId] = useState<string>();
  const [comparisonRunId, setComparisonRunId] = useState("");
  const [comparison, setComparison] = useState<RunComparison>();
  const [category, setCategory] = useState<Category>("all");
  const [query, setQuery] = useState("");
  const [runQuery, setRunQuery] = useState("");
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("payload");
  const [diffMode, setDiffMode] = useState<DiffMode>("unified");
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

  const selectedRun = runs.find((run) => run.run_id === selectedRunId);
  const selectedEvent = events.find((event) => event.event_id === selectedEventId);

  const refreshRuns = useCallback(async () => {
    try {
      const next = await listRuns();
      setRuns(next);
      setSelectedRunId((current) => current ?? next[0]?.run_id);
      setError("");
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const loadSelectedRun = useCallback(async (runId: string, includeRaw: boolean) => {
    const [nextEvents, nextStats, nextArtifacts] = await Promise.all([
      loadEvents(runId, includeRaw),
      loadStats(runId),
      listArtifacts(runId),
    ]);
    setEvents(nextEvents);
    setStats(nextStats);
    setArtifacts(nextArtifacts);
    setSelectedEventId((current) => current && nextEvents.some((event) => event.event_id === current) ? current : nextEvents[0]?.event_id);
  }, []);

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
      setArtifacts([]);
      return;
    }
    let active = true;
    setLoading(true);
    void loadSelectedRun(selectedRunId, rawVisible)
      .then(() => active && setError(""))
      .catch((cause) => active && setError(String(cause)))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [selectedRunId, rawVisible, loadSelectedRun]);

  useEffect(() => {
    if (!selectedRunId || selectedRun?.status !== "running") return;
    let active = true;
    const timer = window.setInterval(() => {
      void loadSelectedRun(selectedRunId, rawVisible)
        .then(() => active && setError(""))
        .catch((cause) => active && setError(String(cause)));
    }, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [selectedRunId, selectedRun?.status, rawVisible, loadSelectedRun]);

  useEffect(() => {
    if (!selectedRun?.harness) {
      setCapabilities(undefined);
      return;
    }
    let active = true;
    void loadCapabilities(selectedRun.harness)
      .then((report) => active && setCapabilities(report))
      .catch((cause) => active && setError(String(cause)));
    return () => { active = false; };
  }, [selectedRun?.harness]);

  useEffect(() => {
    if (!selectedRunId || !comparisonRunId || comparisonRunId === selectedRunId) {
      setComparison(undefined);
      return;
    }
    let active = true;
    void compareRuns(selectedRunId, comparisonRunId)
      .then((result) => active && setComparison(result))
      .catch((cause) => active && setError(String(cause)));
    return () => { active = false; };
  }, [selectedRunId, comparisonRunId]);

  useEffect(() => {
    if (comparisonRunId === selectedRunId) setComparisonRunId("");
  }, [comparisonRunId, selectedRunId]);

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

  const exportSanitized = async () => {
    if (!selectedRunId) return;
    try {
      const body = await exportSanitizedRun(selectedRunId, false);
      const url = URL.createObjectURL(new Blob([body], { type: "application/x-ndjson" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `agenttrace-${selectedRunId}.jsonl`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setError(String(cause));
    }
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
        <ImportPanel
          onImported={async (runId, eventCount) => {
            await refreshRuns();
            if (eventCount > 0) setSelectedRunId(runId);
          }}
          onError={setError}
        />
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
            {selectedRun && <button className="secondary-button" onClick={() => void exportSanitized()}>Export sanitized</button>}
            <ThemeControl />
            <label className="toggle"><input type="checkbox" checked={rawVisible} onChange={(event) => setRawVisible(event.target.checked)} /><span>Raw source</span></label>
            <span className="local-badge">● local only</span>
          </div>
        </header>

        {error && <div className="error-banner">{error}</div>}

        {!selectedRun ? (
          <section className="empty-state">
            <div className="empty-glyph">⌁</div>
            <h2>Record or import an agent trace</h2>
            <p>AgentTrace never invents missing telemetry. Record a supported harness or import an existing structured trace/session from the sidebar.</p>
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

            <TraceCharts stats={stats} events={events} />
            <CapabilityStrip report={capabilities} />
            <ComparisonPanel runs={runs} selectedRunId={selectedRunId!} comparisonRunId={comparisonRunId} setComparisonRunId={setComparisonRunId} comparison={comparison} />

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
                    {inspectorTabs.map((tab) => <button key={tab} className={inspectorTab === tab ? "active" : ""} onClick={() => setInspectorTab(tab)}>{tab}</button>)}
                  </div>
                  {selectedEvent ? (
                    <EventInspector event={selectedEvent} events={events} artifacts={artifacts} tab={inspectorTab} diffMode={diffMode} setDiffMode={setDiffMode} />
                  ) : <div className="empty-inspector">Select an event.</div>}
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

function CapabilityStrip({ report }: { report?: CapabilityReport }) {
  if (!report) return null;
  const entries = Object.entries(report.capabilities).sort(([a], [b]) => a.localeCompare(b));
  return <section className="capability-panel">
    <div className="panel-heading"><span>Capability evidence</span><small>{report.integration_modes.join(" · ")}</small></div>
    <div className="capability-list">
      {entries.map(([name, evidence]) => <span key={name} className={`capability-chip ${evidence.provenance}`} title={`${evidence.source}${evidence.notes ? ` — ${evidence.notes}` : ""}`}>
        <i />{name.replaceAll("_", " ")}<b>{evidence.provenance}</b>
      </span>)}
    </div>
  </section>;
}

function ComparisonPanel({ runs, selectedRunId, comparisonRunId, setComparisonRunId, comparison }: {
  runs: RunSummary[];
  selectedRunId: string;
  comparisonRunId: string;
  setComparisonRunId: (value: string) => void;
  comparison?: RunComparison;
}) {
  const left = comparison?.left.stats;
  const right = comparison?.right.stats;
  return <section className="comparison-panel">
    <div className="comparison-select">
      <span>Compare run</span>
      <select value={comparisonRunId} onChange={(event) => setComparisonRunId(event.target.value)}>
        <option value="">None</option>
        {runs.filter((run) => run.run_id !== selectedRunId).map((run) => <option key={run.run_id} value={run.run_id}>{run.harness} · {compactId(run.run_id)}</option>)}
      </select>
    </div>
    {left && right && <div className="comparison-metrics">
      <CompareMetric label="Events" left={left.event_count} right={right.event_count} />
      <CompareMetric label="Observed tokens" left={aggregateTokens(left)} right={aggregateTokens(right)} />
      <CompareMetric label="Retries" left={left.event_kinds.retry ?? 0} right={right.event_kinds.retry ?? 0} />
      <CompareMetric label="Errors" left={(left.event_kinds.error ?? 0) + (left.event_kinds["run.failed"] ?? 0)} right={(right.event_kinds.error ?? 0) + (right.event_kinds["run.failed"] ?? 0)} />
    </div>}
  </section>;
}

function CompareMetric({ label, left, right }: { label: string; left: number; right: number }) {
  const delta = right - left;
  const format = (value: number) => value.toLocaleString();
  return <div className="compare-metric"><span>{label}</span><code>{format(left)} → {format(right)}</code><small>{delta >= 0 ? "+" : ""}{format(delta)}</small></div>;
}

function EventInspector({ event, events, artifacts, tab, diffMode, setDiffMode }: {
  event: EventEnvelope;
  events: EventEnvelope[];
  artifacts: ArtifactMetadata[];
  tab: InspectorTab;
  diffMode: DiffMode;
  setDiffMode: (mode: DiffMode) => void;
}) {
  if (tab === "raw") return <JsonBlock value={event.raw_source ?? { unavailable: true, reason: "raw source not exposed or hidden" }} />;
  if (tab === "execution") return <ExecutionInspector event={event} />;
  if (tab === "terminal") return <TerminalInspector event={event} />;
  if (tab === "diff") return <DiffInspector event={event} mode={diffMode} setMode={setDiffMode} />;
  if (tab === "relations") return <RelationsInspector selected={event} events={events} artifacts={artifacts} />;
  return <div className="inspector-content">
    <div className="event-title"><span className={`provenance ${event.provenance.level}`}>{event.provenance.level}</span><strong>{event.kind}</strong></div>
    {event.provenance.notes && <p className="evidence-note">{event.provenance.notes}</p>}
    <JsonBlock value={event.payload} />
  </div>;
}

function ExecutionInspector({ event }: { event: EventEnvelope }) {
  return <div className="inspector-content">
    <InspectorField label="Event ID" value={event.event_id} />
    <InspectorField label="Span" value={event.span_id ?? "unavailable"} />
    <InspectorField label="Parent" value={event.parent_span_id ?? "unavailable"} />
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

function TerminalInspector({ event }: { event: EventEnvelope }) {
  const eligible = event.kind.startsWith("shell.") || event.kind === "process.stdout" || event.kind === "process.stderr" || Boolean(event.command);
  if (!eligible) return <div className="inspector-content"><Unavailable message="This event is not terminal/process evidence." /></div>;
  const text = findText(event.payload, ["aggregated_output", "stdout", "stderr", "output", "text", "message"])
    ?? (event.command ? [event.command.program, ...event.command.args].join(" ") : undefined);
  return <div className="inspector-content">
    <h3>Terminal evidence</h3>
    {text ? <pre className="terminal-block">{text}</pre> : <Unavailable message="This terminal event does not expose text." />}
  </div>;
}

function DiffInspector({ event, mode, setMode }: { event: EventEnvelope; mode: DiffMode; setMode: (mode: DiffMode) => void }) {
  const diff = extractDiff(event);
  return <div className="inspector-content">
    <div className="diff-toolbar"><span>Patch evidence</span><div><button className={mode === "unified" ? "active" : ""} onClick={() => setMode("unified")}>Unified</button><button className={mode === "split" ? "active" : ""} onClick={() => setMode("split")}>Side by side</button></div></div>
    {!diff ? <Unavailable message="No unified diff/patch text is exposed on this event." /> : mode === "unified" ? <UnifiedDiff diff={diff} /> : <SplitDiff diff={diff} />}
  </div>;
}

function UnifiedDiff({ diff }: { diff: string }) {
  return <pre className="diff-block">{diff.split("\n").map((line, index) => <span key={index} className={diffLineClass(line)}>{line || " "}{"\n"}</span>)}</pre>;
}

function SplitDiff({ diff }: { diff: string }) {
  const rows = splitDiffRows(diff);
  return <div className="split-diff">
    <div className="split-head"><span>Before</span><span>After</span></div>
    {rows.map((row, index) => <div className="split-row" key={index}><code className={row.leftClass}>{row.left || " "}</code><code className={row.rightClass}>{row.right || " "}</code></div>)}
  </div>;
}

function RelationsInspector({ selected, events, artifacts }: { selected: EventEnvelope; events: EventEnvelope[]; artifacts: ArtifactMetadata[] }) {
  const spans = buildSpanRows(events);
  const subagents = events.filter((event) => event.kind.startsWith("subagent."));
  const contexts = events.filter((event) => event.kind.startsWith("context."));
  const linkedArtifacts = artifacts.filter((artifact) => !artifact.event_id || artifact.event_id === selected.event_id);
  return <div className="inspector-content relation-view">
    <h3>Span tree</h3>
    {spans.length ? <div className="relation-list">{spans.map((row) => <div key={row.spanId} className={row.spanId === selected.span_id ? "relation-row selected-relation" : "relation-row"} style={{ paddingLeft: 8 + row.depth * 14 }}><code>{compactId(row.spanId)}</code><span>{row.label}</span><small>{row.count} event{row.count === 1 ? "" : "s"}</small></div>)}</div> : <Unavailable message="This run does not expose span IDs." />}
    <h3>Subagents</h3>
    {subagents.length ? <RelationEvents events={subagents} /> : <Unavailable message="No subagent lifecycle events are exposed for this run." />}
    <h3>Context provenance</h3>
    {contexts.length ? <RelationEvents events={contexts} /> : <Unavailable message="No explicit context lifecycle events are exposed for this run." />}
    <h3>Artifacts</h3>
    {linkedArtifacts.length ? <div className="artifact-list">{linkedArtifacts.map((artifact) => <div key={artifact.artifact_id}><strong>{artifact.name}</strong><span>{artifact.kind} · {artifact.original_size.toLocaleString()} bytes</span><code>sha256:{artifact.content_sha256.slice(0, 16)}…</code></div>)}</div> : <Unavailable message="No stored artifact metadata is linked to this event/run." />}
  </div>;
}

function RelationEvents({ events }: { events: EventEnvelope[] }) {
  return <div className="relation-list">{events.map((event) => <div key={event.event_id} className="relation-row"><code>#{event.sequence}</code><span>{event.kind}</span><small className={`provenance ${event.provenance.level}`}>{event.provenance.level}</small></div>)}</div>;
}

function InspectorField({ label, value }: { label: string; value: string }) {
  return <div className="inspector-field"><span>{label}</span><code>{value}</code></div>;
}

function JsonBlock({ value }: { value: unknown }) {
  return <pre className="json-block">{JSON.stringify(value, null, 2)}</pre>;
}

function Unavailable({ message }: { message: string }) {
  return <div className="unavailable-box">unavailable · {message}</div>;
}

function findText(value: unknown, keys: string[], depth = 0): string | undefined {
  if (depth > 4 || value == null) return undefined;
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findText(item, keys, depth + 1);
      if (found) return found;
    }
    return undefined;
  }
  if (typeof value === "object") {
    const object = value as Record<string, unknown>;
    for (const key of keys) {
      if (typeof object[key] === "string") return object[key] as string;
    }
    for (const child of Object.values(object)) {
      const found = findText(child, keys, depth + 1);
      if (found) return found;
    }
  }
  return undefined;
}

function extractDiff(event: EventEnvelope): string | undefined {
  const keys = ["unified_diff", "patch", "diff"];
  const payloadDiff = findText(event.payload, keys);
  if (payloadDiff && looksLikeDiff(payloadDiff)) return payloadDiff;
  const rawDiff = findText(event.raw_source?.data, keys);
  if (rawDiff && looksLikeDiff(rawDiff)) return rawDiff;
  return undefined;
}

function looksLikeDiff(value: string) {
  return value.includes("@@") || (value.includes("\n+") && value.includes("\n-")) || value.startsWith("diff --git");
}

function diffLineClass(line: string) {
  if (line.startsWith("@@")) return "diff-hunk";
  if (line.startsWith("+++ ") || line.startsWith("--- ") || line.startsWith("diff --git")) return "diff-meta";
  if (line.startsWith("+")) return "diff-add";
  if (line.startsWith("-")) return "diff-del";
  return "diff-context";
}

type SplitRow = { left: string; right: string; leftClass: string; rightClass: string };

function splitDiffRows(diff: string): SplitRow[] {
  const rows: SplitRow[] = [];
  const deleted: string[] = [];
  const added: string[] = [];
  const flush = () => {
    const count = Math.max(deleted.length, added.length);
    for (let index = 0; index < count; index += 1) rows.push({ left: deleted[index] ?? "", right: added[index] ?? "", leftClass: "diff-del", rightClass: "diff-add" });
    deleted.length = 0;
    added.length = 0;
  };
  for (const line of diff.split("\n")) {
    if (line.startsWith("--- ") || line.startsWith("+++ ") || line.startsWith("diff --git") || line.startsWith("index ")) {
      flush();
      rows.push({ left: line, right: line, leftClass: "diff-meta", rightClass: "diff-meta" });
    } else if (line.startsWith("@@")) {
      flush();
      rows.push({ left: line, right: line, leftClass: "diff-hunk", rightClass: "diff-hunk" });
    } else if (line.startsWith("-") && !line.startsWith("---")) deleted.push(line.slice(1));
    else if (line.startsWith("+") && !line.startsWith("+++")) added.push(line.slice(1));
    else {
      flush();
      const context = line.startsWith(" ") ? line.slice(1) : line;
      rows.push({ left: context, right: context, leftClass: "diff-context", rightClass: "diff-context" });
    }
  }
  flush();
  return rows;
}

type SpanRow = { spanId: string; depth: number; label: string; count: number };

function buildSpanRows(events: EventEnvelope[]): SpanRow[] {
  const groups = new Map<string, EventEnvelope[]>();
  for (const event of events) {
    if (!event.span_id) continue;
    const group = groups.get(event.span_id) ?? [];
    group.push(event);
    groups.set(event.span_id, group);
  }
  if (!groups.size) return [];
  const parentBySpan = new Map<string, string | undefined>();
  for (const [spanId, group] of groups) parentBySpan.set(spanId, group.find((event) => event.parent_span_id)?.parent_span_id);
  const children = new Map<string, string[]>();
  const roots: string[] = [];
  for (const spanId of groups.keys()) {
    const parent = parentBySpan.get(spanId);
    if (!parent || !groups.has(parent)) roots.push(spanId);
    else children.set(parent, [...(children.get(parent) ?? []), spanId]);
  }
  const rows: SpanRow[] = [];
  const seen = new Set<string>();
  const visit = (spanId: string, depth: number) => {
    if (seen.has(spanId)) return;
    seen.add(spanId);
    const group = groups.get(spanId) ?? [];
    rows.push({ spanId, depth, label: group[0]?.kind ?? "span", count: group.length });
    for (const child of children.get(spanId) ?? []) visit(child, depth + 1);
  };
  for (const root of roots) visit(root, 0);
  for (const spanId of groups.keys()) visit(spanId, 0);
  return rows;
}
