import { pr } from "./api";
import type { CiCheck, PrStatus } from "./types";
import { createPoller } from "./poller.svelte";

export type CiConclusion = "pass" | "fail" | "pending";

export function classifyCheck(check: CiCheck): CiConclusion {
  if (
    check.conclusion === "success" ||
    check.conclusion === "neutral" ||
    check.conclusion === "skipped"
  )
    return "pass";
  if (
    check.conclusion === "failure" ||
    check.conclusion === "cancelled" ||
    check.conclusion === "timed_out"
  )
    return "fail";
  return "pending";
}

export type CiOverall = "passing" | "failing" | "running" | null;

function deriveOverall(checks: CiCheck[]): CiOverall {
  if (checks.length === 0) return null;
  if (checks.some((c) => classifyCheck(c) === "fail")) return "failing";
  if (checks.some((c) => classifyCheck(c) === "pending")) return "running";
  return "passing";
}

const poller = createPoller<PrStatus>({
  fetch: pr.getPrStatus,
  shouldSkip: (_id, current) =>
    !!current && current.checks.length > 0 && current.checks.every((c) => c.conclusion !== null) && !current.conflicting,
  onUpdateSessions: (state) => {
    let changed = false;
    const next = { ...state };
    for (const [id, status] of Object.entries(next)) {
      if (status && status.checks.length > 0 && status.checks.every((c) => c.conclusion !== null)) {
        delete next[id];
        changed = true;
      }
    }
    return changed ? next : state;
  },
});

export const getCiChecks = (sessionId: string): CiCheck[] => poller.get(sessionId)?.checks ?? [];
export const getCiStatus = (sessionId: string): CiOverall => deriveOverall(getCiChecks(sessionId));
export const hasConflicts = (sessionId: string): boolean => poller.get(sessionId)?.conflicting ?? false;
export const refreshCiChecks = poller.refresh;
export const startPolling = poller.startPolling;
export const updateSessions = poller.updateSessions;
