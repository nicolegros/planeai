export interface TerminalEditorQueue {
  sessionId: string;
  filePath: string;
  getTerminalCommand: (sessionId: string, filePath: string) => Promise<string>;
  getFocusedLeafId: () => string | null;
  addTab: (sessionId: string) => number;
  removeTab: (sessionId: string, tabIndex: number) => void;
  incrementTabCount: (sessionId: string) => Promise<unknown>;
  closeTab: (sessionId: string, tabIndex: number) => Promise<unknown>;
  /** Returns whether the tab reached the layout; false means roll back. */
  addShellTab: (leafId: string, ptyKey: string, label: string) => boolean;
  pendingCommands: Map<string, string>;
}

/**
 * Create a dedicated shell tab for a terminal editor command.
 *
 * The command is reserved against its pty key before the tab count is persisted,
 * so a workspace reconcile that observes the new session tab mid-flight skips it
 * instead of mounting a bare shell. The persisted tab count is still updated
 * before the shell tab mounts, so closing the tab cannot race ahead of the count
 * increment.
 *
 * Both failure paths leave no reservation, no local tab and no mounted terminal:
 * a failed persistence update, and a target pane that the user closed or
 * navigated away from while the update was in flight.
 */
export async function queueTerminalEditor(queue: TerminalEditorQueue): Promise<boolean> {
  const focusedLeafId = queue.getFocusedLeafId();
  if (!focusedLeafId) return false;

  const command = await queue.getTerminalCommand(queue.sessionId, queue.filePath);
  const tabIndex = queue.addTab(queue.sessionId);
  if (tabIndex === -1) return false;

  // Record the command before the first await. `addTab` mutates shared tab state,
  // which schedules the workspace reconciliation effect; that effect runs during
  // the await below and can mount the terminal for this tab itself. If the command
  // were registered afterwards the terminal would spawn a bare shell instead of
  // the editor.
  const ptyKey = `${queue.sessionId}:${tabIndex}`;
  queue.pendingCommands.set(ptyKey, command);

  try {
    await queue.incrementTabCount(queue.sessionId);
  } catch (error) {
    queue.pendingCommands.delete(ptyKey);
    queue.removeTab(queue.sessionId, tabIndex);
    throw error;
  }

  const label = queue.filePath.split("/").pop() ?? queue.filePath;
  if (!queue.addShellTab(focusedLeafId, ptyKey, label)) {
    // The pane is gone. Release the reservation and the count that was just
    // persisted, otherwise the tab is unreachable and uncloseable. A failed
    // release rejects so the caller can report the drifted tab count.
    await rollbackPendingTerminalEditor({
      ptyKey,
      pendingCommands: queue.pendingCommands,
      removeTab: queue.removeTab,
      closeTab: queue.closeTab,
    });
    return false;
  }
  return true;
}

export interface TerminalEditorRollback {
  ptyKey: string;
  pendingCommands: Map<string, string>;
  removeTab: (sessionId: string, tabIndex: number) => void;
  closeTab: (sessionId: string, tabIndex: number) => Promise<unknown>;
}

/**
 * Release a terminal editor tab that will never run: its shell PTY failed to
 * attach, or its target pane vanished before the tab could be placed.
 *
 * Returns false when the pty key held no reservation. Rejects if the backend tab
 * cannot be closed, leaving the persisted tab count drifted — the caller must
 * report that rather than swallow it.
 */
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

export interface PendingTerminalEditorCommand {
  ptyKey: string;
  pendingCommands: Map<string, string>;
}

/** Return the editor command that must be passed while its shell PTY is spawned. */
export function getPendingTerminalEditorCommand(
  pending: PendingTerminalEditorCommand,
): string | undefined {
  return pending.pendingCommands.get(pending.ptyKey);
}

/** Clear a queued editor command after its shell PTY started successfully. */
export function consumePendingTerminalEditorCommand(
  pending: PendingTerminalEditorCommand,
): boolean {
  return pending.pendingCommands.delete(pending.ptyKey);
}
