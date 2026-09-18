import type { EventEnvelope, RunStats } from "../types";

type BarDatum = {
  label: string;
  value: number;
  display: string;
};

type Props = {
  stats?: RunStats;
  events: EventEnvelope[];
};

function BarChart({ title, data, empty }: { title: string; data: BarDatum[]; empty: string }) {
  const max = Math.max(...data.map((item) => item.value), 0);
  return (
    <div className="chart-card">
      <div className="chart-heading"><span>{title}</span></div>
      {!data.length ? <div className="chart-empty">unavailable · {empty}</div> : (
        <div className="bar-chart">
          {data.map((item) => (
            <div className="bar-row" key={item.label}>
              <span title={item.label}>{item.label}</span>
              <div className="bar-track"><i style={{ width: `${max ? Math.max(2, (item.value / max) * 100) : 0}%` }} /></div>
              <code>{item.display}</code>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function tokenData(stats?: RunStats): BarDatum[] {
  if (!stats) return [];
  const values: Array<[string, number]> = [
    ["input", stats.tokens.input],
    ["output", stats.tokens.output],
    ["cached input", stats.tokens.cached_input],
    ["reasoning", stats.tokens.reasoning_output],
  ];
  return values
    .filter(([, value]) => value > 0)
    .map(([label, value]) => ({ label, value, display: value.toLocaleString() }));
}

function eventData(stats?: RunStats): BarDatum[] {
  if (!stats) return [];
  return Object.entries(stats.event_kinds)
    .filter(([, value]) => value > 0)
    .sort((left, right) => right[1] - left[1])
    .slice(0, 8)
    .map(([label, value]) => ({ label, value, display: value.toLocaleString() }));
}

function durationData(events: EventEnvelope[]): BarDatum[] {
  return timingData(events, "duration_ns");
}

function latencyData(events: EventEnvelope[]): BarDatum[] {
  return timingData(events, "latency_ns");
}

function timingData(events: EventEnvelope[], field: "duration_ns" | "latency_ns"): BarDatum[] {
  return events
    .filter((event) => (event[field] ?? 0) > 0)
    .sort((left, right) => (right[field] ?? 0) - (left[field] ?? 0))
    .slice(0, 8)
    .map((event) => {
      const nanoseconds = event[field] ?? 0;
      const milliseconds = nanoseconds / 1_000_000;
      const display = milliseconds < 1000 ? `${milliseconds.toFixed(1)} ms` : `${(milliseconds / 1000).toFixed(2)} s`;
      return {
        label: `#${event.sequence} ${event.kind}`,
        value: nanoseconds,
        display,
      };
    });
}

export function TraceCharts({ stats, events }: Props) {
  return (
    <section className="chart-grid" aria-label="Observed trace charts">
      <BarChart title="Token usage" data={tokenData(stats)} empty="no token usage was exposed" />
      <BarChart title="API latency" data={latencyData(events)} empty="no explicit latency was exposed" />
      <BarChart title="Event distribution" data={eventData(stats)} empty="no normalized events" />
      <BarChart title="Longest observed durations" data={durationData(events)} empty="no event durations were exposed" />
    </section>
  );
}
