/**
 * Canonical loop status type and helpers.
 *
 * Single source of truth for loop status semantics on the frontend.
 * Adding a backend status requires extending LoopStatusValue — the compiler
 * will flag every switch/map that needs updating.
 */

export type LoopStatusValue =
  | "draft"
  | "running"
  | "observing"
  | "verifying"
  | "completed_unreviewed"
  | "blocked"
  | "needs_human"
  | "stale"
  | "failed"
  | "cancelled"
  | "approved"
  | "merged"
  | "cleaned";

// ─── Predicates ──────────────────────────────────────────────────────────────

/** Status indicates the loop is actively executing (tick-able). */
export function isActive(s: LoopStatusValue): boolean {
  return s === "running" || s === "observing" || s === "verifying";
}

/** Status indicates the loop has reached an end state. */
export function isTerminal(s: LoopStatusValue): boolean {
  return (
    s === "failed" || s === "cancelled" || s === "approved" || s === "merged" || s === "cleaned"
  );
}

/** Status indicates the loop needs human attention before it can proceed. */
export function isInterventionRequired(s: LoopStatusValue): boolean {
  return s === "blocked" || s === "needs_human" || s === "completed_unreviewed" || s === "stale";
}

// ─── Action guards ───────────────────────────────────────────────────────────

/** Whether the loop can be ticked (advance one step). */
export function canTick(s: LoopStatusValue): boolean {
  return isActive(s);
}

/** Whether the loop can be stopped. */
export function canStop(s: LoopStatusValue): boolean {
  return isActive(s);
}

/** Whether the loop can be started. */
export function canStart(s: LoopStatusValue): boolean {
  return s === "draft";
}

// ─── Display helpers ─────────────────────────────────────────────────────────

/** Tailwind background color class for sidebar status dots. */
export function statusColor(s: LoopStatusValue): string {
  const colors: Record<LoopStatusValue, string> = {
    draft: "bg-t3",
    running: "bg-status-running",
    observing: "bg-status-running",
    verifying: "bg-status-running",
    completed_unreviewed: "bg-status-review",
    blocked: "bg-status-exited",
    needs_human: "bg-status-review",
    stale: "bg-status-exited",
    failed: "bg-status-exited",
    cancelled: "bg-status-exited",
    approved: "bg-status-running",
    merged: "bg-status-idle",
    cleaned: "bg-status-idle",
  };
  return colors[s];
}

/** Tailwind badge classes (bg + text) for the dashboard badge. */
export function statusBadgeColor(s: LoopStatusValue): string {
  const colors: Record<LoopStatusValue, string> = {
    draft: "bg-t3/20 text-t2",
    running: "bg-status-running/20 text-status-running",
    observing: "bg-status-running/20 text-status-running",
    verifying: "bg-status-running/20 text-status-running",
    completed_unreviewed: "bg-status-review/20 text-status-review",
    blocked: "bg-status-exited/20 text-status-exited",
    needs_human: "bg-status-review/20 text-status-review",
    stale: "bg-status-exited/20 text-status-exited",
    failed: "bg-status-exited/20 text-status-exited",
    cancelled: "bg-status-exited/20 text-status-exited",
    approved: "bg-status-running/20 text-status-running",
    merged: "bg-status-idle/20 text-status-idle",
    cleaned: "bg-status-idle/20 text-status-idle",
  };
  return colors[s];
}

/** Human-readable label for the status. */
export function statusLabel(s: LoopStatusValue): string {
  const labels: Record<LoopStatusValue, string> = {
    draft: "Draft",
    running: "Running",
    observing: "Observing",
    verifying: "Verifying",
    completed_unreviewed: "Needs Review",
    blocked: "Blocked",
    needs_human: "Needs Human",
    stale: "Stale",
    failed: "Failed",
    cancelled: "Cancelled",
    approved: "Approved",
    merged: "Merged",
    cleaned: "Cleaned",
  };
  return labels[s];
}
