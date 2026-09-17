export type RunSummary = {
  run_id: string;
  trace_id: string;
  harness: string;
  integration_mode: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  last_sequence: number;
  event_count: number;
};

export type Provenance = {
  level: "native" | "inferred" | "derived" | "unavailable";
  source: string;
  native_fields?: string[];
  unavailable_fields?: string[];
  notes?: string;
};

export type TokenUsage = {
  input_tokens?: number;
  output_tokens?: number;
  cached_input_tokens?: number;
  cache_write_input_tokens?: number;
  reasoning_output_tokens?: number;
};

export type EventEnvelope = {
  schema_version: number;
  event_id: string;
  run_id: string;
  trace_id: string;
  span_id?: string;
  parent_span_id?: string;
  sequence: number;
  timestamp: string;
  monotonic_ns?: number;
  harness: string;
  integration_mode: string;
  provenance: Provenance;
  kind: string;
  payload: unknown;
  raw_source?: { source: string; media_type: string; data: unknown };
  model?: { id: string; provider?: string };
  usage?: TokenUsage;
  cost?: { amount: number; currency: string; basis: string; price_table_version?: string };
  duration_ns?: number;
  command?: { program: string; args: string[]; cwd?: string; exit_code?: number };
  filesystem_impact?: {
    paths_read?: string[];
    paths_written?: string[];
    paths_created?: string[];
    paths_deleted?: string[];
  };
  error?: { message: string; code?: string; recoverable?: boolean };
  attributes?: Record<string, unknown>;
};

export type RunStats = {
  event_count: number;
  event_kinds: Record<string, number>;
  provenance: Record<string, number>;
  tokens: {
    input: number;
    output: number;
    cached_input: number;
    reasoning_output: number;
  };
  reported_or_deterministic_cost_by_currency: Record<string, number>;
  sum_event_duration_ns: number;
};

export type CapabilityEvidence = {
  provenance: Provenance["level"];
  source: string;
  notes?: string;
};

export type CapabilityReport = {
  harness: string;
  integration_modes: string[];
  capabilities: Record<string, CapabilityEvidence>;
};

export type RunComparison = {
  left: { run_id: string; stats: RunStats };
  right: { run_id: string; stats: RunStats };
};

export type ArtifactMetadata = {
  artifact_id: string;
  run_id: string;
  event_id?: string;
  name: string;
  kind: string;
  media_type?: string;
  content_sha256: string;
  original_size: number;
  compressed: boolean;
  created_at: string;
};
