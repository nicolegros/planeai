import { pr } from "./api";
import type { CiCheck } from "./types";

let ciChecks = $state<CiCheck[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;
let activeSessionId: string | null = null;
let activePrUrl: string | null = null;

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

export function getCiChecks(): CiCheck[] {
  return ciChecks;
}

export function refreshCiChecks(): void {
  if (activeSessionId && activePrUrl) fetchChecks(activeSessionId);
}

export function startPolling(sessionId: string | null, prUrl: string | null): void {
  if (sessionId === activeSessionId && prUrl === activePrUrl) return;
  stopPolling();
  activeSessionId = sessionId;
  activePrUrl = prUrl;
  ciChecks = [];
  if (!sessionId || !prUrl) return;
  fetchChecks(sessionId);
  pollTimer = setInterval(() => {
    if (!activeSessionId || !activePrUrl || allConcluded()) {
      stopPolling();
      return;
    }
    fetchChecks(activeSessionId);
  }, 30_000);
}

export function stopPolling(): void {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function allConcluded(): boolean {
  return (
    ciChecks.length > 0 && ciChecks.every((c) => c.status === "completed" || c.conclusion !== null)
  );
}

async function fetchChecks(sessionId: string): Promise<void> {
  try {
    ciChecks = await pr.getCiChecks(sessionId);
  } catch {
    // gh not available or no checks
  }
}
