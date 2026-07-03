const SHELLS = new Set(["sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh"]);

/**
 * Extracts a command binary name from an OSC title string.
 * Returns null if the title looks like a shell reset (not a user command).
 */
export function extractCommandName(oscTitle: string): string | null {
  if (!oscTitle || !oscTitle.trim()) return null;

  const trimmed = oscTitle.trim();

  // Reject tilde-based paths (always a cwd from shell precmd)
  if (trimmed.startsWith("~")) return null;

  // Reject user@host:path patterns (common shell title format)
  if (/^[\w-]+@[\w.-]+:/.test(trimmed)) return null;

  // Reject bare "/" or paths ending in "/"
  if (trimmed === "/" || trimmed.endsWith("/")) return null;

  // Extract last path segment (handles both "/usr/bin/vim" and "vim")
  const lastSegment = trimmed.includes("/")
    ? trimmed.split("/").pop()!
    : trimmed;

  if (!lastSegment) return null;

  // Ignore shell names (with optional leading dash for login shells)
  const normalized = lastSegment.replace(/^-/, "");
  if (SHELLS.has(normalized)) return null;

  return lastSegment;
}
