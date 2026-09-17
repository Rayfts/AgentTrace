import { invoke } from "@tauri-apps/api/core";
import type {
  ArtifactMetadata,
  CapabilityReport,
  EventEnvelope,
  ImportResult,
  RunComparison,
  RunStats,
  RunSummary,
} from "../types";

export async function listRuns(limit = 250): Promise<RunSummary[]> {
  return invoke<RunSummary[]>("list_runs", { limit });
}

export async function loadRun(runId: string): Promise<RunSummary> {
  return invoke<RunSummary>("get_run", { runId });
}

export async function loadEvents(runId: string, raw = true): Promise<EventEnvelope[]> {
  return invoke<EventEnvelope[]>("get_run_events", { runId, raw });
}

export async function loadStats(runId: string): Promise<RunStats> {
  return invoke<RunStats>("get_run_stats", { runId });
}

export async function importTrace(harness: string, path: string): Promise<ImportResult> {
  return invoke<ImportResult>("import_trace", { harness, path });
}

export async function loadCapabilities(harness: string): Promise<CapabilityReport> {
  return invoke<CapabilityReport>("get_harness_capabilities", { harness });
}

export async function compareRuns(left: string, right: string): Promise<RunComparison> {
  return invoke<RunComparison>("compare_runs", { left, right });
}

export async function exportSanitizedRun(runId: string, raw = false): Promise<string> {
  return invoke<string>("export_run_sanitized", { runId, raw });
}

export async function listArtifacts(runId: string): Promise<ArtifactMetadata[]> {
  return invoke<ArtifactMetadata[]>("list_run_artifacts", { runId });
}

export async function databaseLocation(): Promise<string> {
  return invoke<string>("database_location");
}
