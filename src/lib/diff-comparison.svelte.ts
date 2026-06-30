/**
 * Per-session in-memory diff comparison state.
 * Tracks what base ref and head ref the user is comparing in the diff view.
 * Resets on app restart (no persistence).
 */

export interface DiffComparison {
  /** Base ref (branch name or commit SHA). Defaults to session's base_branch. */
  baseRef: string;
  /** Head ref (commit SHA, branch, or null for working tree). null = working tree. */
  headRef: string | null;
}

// Map of sessionId → comparison state
let comparisons = $state<Map<string, DiffComparison>>(new Map());

/**
 * Get the current comparison for a session.
 * Returns the custom comparison if set, otherwise returns default.
 */
export function getComparison(sessionId: string, defaultBase: string): DiffComparison {
  return comparisons.get(sessionId) ?? { baseRef: defaultBase, headRef: null };
}

/**
 * Set the comparison for a session.
 */
export function setComparison(sessionId: string, comparison: DiffComparison): void {
  comparisons.set(sessionId, comparison);
  // Trigger reactivity by reassigning
  comparisons = new Map(comparisons);
}

/**
 * Reset a session's comparison to defaults.
 */
export function resetComparison(sessionId: string): void {
  comparisons.delete(sessionId);
  comparisons = new Map(comparisons);
}

/**
 * Check if a session has a custom (non-default) comparison set.
 */
export function hasCustomComparison(sessionId: string): boolean {
  return comparisons.has(sessionId);
}

/**
 * Format the comparison as a display string (e.g. "main..Working tree").
 */
export function formatComparison(comparison: DiffComparison): string {
  const head = comparison.headRef ?? "Working tree";
  return `${comparison.baseRef}..${head}`;
}
