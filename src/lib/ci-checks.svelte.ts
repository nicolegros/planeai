import { pr } from "./api";
import type { CiCheck, Session } from "./types";

let ciChecks = $state<Record<string, CiCheck[]>>({});
let pollTimer: ReturnType<typeof setInterval> | null = null;
let activeSessions: Session[] = [];

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

export function getCiChecks(sessionId: string): CiCheck[] {
  return ciChecks[sessionId] ?? [];
}

export function getCiStatus(sessionId: string): CiOverall {
  return deriveOverall(ciChecks[sessionId] ?? []);
}

export function refreshCiChecks(sessionId: string): void {
  fetchChecks(sessionId);
}

export function startPolling(sessions: Session[]): () => void {
  activeSessions = sessions;
  pollAll();
  pollTimer = setInterval(pollAll, 60_000);
  let lastFocusPoll = 0;
  const onFocus = () => {
    const now = Date.now();
    if (now - lastFocusPoll < 5_000) return;
    lastFocusPoll = now;
    pollAll();
  };
  window.addEventListener("focus", onFocus);
  return () => {
    stopPolling();
    window.removeEventListener("focus", onFocus);
  };
}

export function updateSessions(sessions: Session[]): void {
  activeSessions = sessions;
  // Clear concluded checks so next poll re-fetches (handles agent push → new CI run)
  let invalidated = false;
  for (const id of Object.keys(ciChecks)) {
    const checks = ciChecks[id];
    if (checks && checks.length > 0 && checks.every((c) => c.conclusion !== null)) {
      delete ciChecks[id];
      invalidated = true;
    }
  }
  if (invalidated) ciChecks = { ...ciChecks };
}

function stopPolling(): void {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function pollAll(): Promise<void> {
  const targets = activeSessions.filter((s) => {
    if (s.status !== "active" || !s.pr_url) return false;
    const existing = ciChecks[s.id];
    // Skip if all checks already concluded
    if (existing && existing.length > 0 && existing.every((c) => c.conclusion !== null))
      return false;
    return true;
  });
  if (targets.length === 0) return;
  const results = await Promise.allSettled(
    targets.map(async (s) => {
      const checks = await pr.getCiChecks(s.id);
      return { id: s.id, checks };
    }),
  );
  const next: Record<string, CiCheck[]> = { ...ciChecks };
  for (const r of results) {
    if (r.status === "fulfilled") {
      next[r.value.id] = r.value.checks;
    }
  }
  ciChecks = next;
}

async function fetchChecks(sessionId: string): Promise<void> {
  try {
    const checks = await pr.getCiChecks(sessionId);
    ciChecks = { ...ciChecks, [sessionId]: checks };
  } catch {
    // gh not available or no checks
  }
}
