export interface TerminalEditorQueue {
  sessionId: string;
  filePath: string;
  getTerminalCommand: (sessionId: string, filePath: string) => Promise<string>;
  getFocusedLeafId: () => string | null;
  addTab: (sessionId: string) => number;
  removeTab: (sessionId: string, tabIndex: number) => void;
  incrementTabCount: (sessionId: string) => Promise<unknown>;
  addShellTab: (leafId: string, ptyKey: string, label: string) => void;
  pendingCommands: Map<string, string>;
}

/**
 * Create a dedicated shell tab for a terminal editor command.
 *
 * The persisted tab count is updated before the shell tab mounts, so closing the
 * tab cannot race ahead of the count increment. A failed persistence update
 * removes the local tab and leaves no pending command or mounted terminal.
 */
export async function queueTerminalEditor(queue: TerminalEditorQueue): Promise<boolean> {
  const focusedLeafId = queue.getFocusedLeafId();
  if (!focusedLeafId) return false;

  const command = await queue.getTerminalCommand(queue.sessionId, queue.filePath);
  const tabIndex = queue.addTab(queue.sessionId);
  if (tabIndex === -1) return false;

  try {
    await queue.incrementTabCount(queue.sessionId);
  } catch (error) {
    queue.removeTab(queue.sessionId, tabIndex);
    throw error;
  }

  const ptyKey = `${queue.sessionId}:${tabIndex}`;
  queue.pendingCommands.set(ptyKey, command);
  queue.addShellTab(focusedLeafId, ptyKey, queue.filePath.split("/").pop() ?? queue.filePath);
  return true;
}

export interface TerminalEditorRollback {
  ptyKey: string;
  pendingCommands: Map<string, string>;
  removeTab: (sessionId: string, tabIndex: number) => void;
  closeTab: (sessionId: string, tabIndex: number) => Promise<unknown>;
}

/** Remove a terminal editor tab after its shell PTY fails to attach. */
export async function rollbackPendingTerminalEditor(
  rollback: TerminalEditorRollback,
): Promise<boolean> {
  if (!rollback.pendingCommands.delete(rollback.ptyKey)) return false;

  const separator = rollback.ptyKey.lastIndexOf(":");
  const sessionId = rollback.ptyKey.slice(0, separator);
  const tabIndex = Number.parseInt(rollback.ptyKey.slice(separator + 1), 10);
  if (!sessionId || Number.isNaN(tabIndex)) return false;

  rollback.removeTab(sessionId, tabIndex);
  await rollback.closeTab(sessionId, tabIndex);
  return true;
}

export interface PendingTerminalEditorDelivery {
  ptyKey: string;
  pendingCommands: Map<string, string>;
  write: (ptyKey: string, data: number[]) => Promise<boolean>;
}

/** Deliver a queued terminal editor command only after its PTY reports readiness. */
export async function deliverPendingTerminalEditor(
  delivery: PendingTerminalEditorDelivery,
): Promise<"none" | "delivered" | "unavailable"> {
  const command = delivery.pendingCommands.get(delivery.ptyKey);
  if (!command) return "none";

  const delivered = await delivery.write(
    delivery.ptyKey,
    Array.from(new TextEncoder().encode(`${command}\r`)),
  );
  if (!delivered) return "unavailable";

  delivery.pendingCommands.delete(delivery.ptyKey);
  return "delivered";
}
