import { invoke } from "@tauri-apps/api/core";
import type { EventEnvelope, RunStats, RunSummary } from "../types";

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

export async function databaseLocation(): Promise<string> {
  return invoke<string>("database_location");
}
