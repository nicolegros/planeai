/**
 * Pty key — the canonical identity of a TaskWorkspace resource tab.
 *
 * The grammar is shared with the backend and persisted in layouts, so it must
 * not change:
 * - agent: `<sessionId>`
 * - shell: `<sessionId>:<index>` (index ≥ 1)
 * - diff: `<sessionId>:diff`
 * - editor: `<sessionId>:editor:<filePath>`
 */

export type PtyKeyParts =
  | { kind: "agent"; sessionId: string }
  | { kind: "shell"; sessionId: string; index: number }
  | { kind: "diff"; sessionId: string }
  | { kind: "editor"; sessionId: string; filePath: string };

const EDITOR_INFIX = ":editor:";

export function agentPtyKey(sessionId: string): string {
  return sessionId;
}

export function shellPtyKey(sessionId: string, index: number): string {
  return `${sessionId}:${index}`;
}

export function diffPtyKey(sessionId: string): string {
  return `${sessionId}:diff`;
}

export function editorPtyKey(sessionId: string, filePath: string): string {
  return `${sessionId}${EDITOR_INFIX}${filePath}`;
}

/** Parse a pty key, or null when it does not follow the grammar. */
export function parsePtyKey(ptyKey: string): PtyKeyParts | null {
  const separator = ptyKey.indexOf(":");
  if (separator === -1) return ptyKey ? { kind: "agent", sessionId: ptyKey } : null;
  const sessionId = ptyKey.slice(0, separator);
  if (!sessionId) return null;
  const rest = ptyKey.slice(separator + 1);
  if (rest === "diff") return { kind: "diff", sessionId };
  if (ptyKey.startsWith(EDITOR_INFIX, separator)) {
    const filePath = ptyKey.slice(separator + EDITOR_INFIX.length);
    return filePath ? { kind: "editor", sessionId, filePath } : null;
  }
  if (!/^[1-9]\d*$/.test(rest)) return null;
  return { kind: "shell", sessionId, index: Number(rest) };
}

/** The agent session a resource belongs to. */
export function ptyKeySessionId(ptyKey: string): string {
  const separator = ptyKey.indexOf(":");
  return separator === -1 ? ptyKey : ptyKey.slice(0, separator);
}

/** Shell tab index of a pty key, or null when it is not a shell tab. */
export function shellIndex(ptyKey: string): number | null {
  const parts = parsePtyKey(ptyKey);
  return parts?.kind === "shell" ? parts.index : null;
}
