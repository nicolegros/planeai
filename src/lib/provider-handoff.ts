import type { TabClose } from "./task-workspace-layout.svelte";

/** A terminal that exits sooner than this after opening most likely failed to start the TUI. */
const QUICK_EXIT_MS = 5_000;

export interface ProviderHandoffDeps {
  api: {
    handoff(sessionId: string): Promise<string[]>;
    handback(sessionId: string): Promise<void>;
  };
  /** Closes a terminal's tab in the shown layout; false when that layout does not hold it. */
  discardTab(ptyKey: string): Promise<boolean>;
  /** Ends a terminal's process when its tab is in a layout not loaded. */
  closeTerminal(ptyKey: string): Promise<void>;
  notify(message: string): void;
  now?: () => number;
}

/**
 * Continues provider sessions in their agent's TUI. The provider detaches first, so
 * only one side ever drives the conversation; closing the tab hands it back.
 */
export function createProviderHandoff({
  api,
  discardTab,
  closeTerminal,
  notify,
  now = Date.now,
}: ProviderHandoffDeps) {
  /** Terminal tabs continuing a provider session, keyed by pty key. One per session. */
  const tabs = new Map<string, { sessionId: string; openedAt: number }>();
  /** Handoffs still asking the provider for its command, so a repeat request joins them. */
  const starting = new Map<string, Promise<string>>();

  function tabFor(sessionId: string): string | undefined {
    for (const [ptyKey, tab] of tabs) if (tab.sessionId === sessionId) return ptyKey;
    return undefined;
  }

  async function openTab(
    sessionId: string,
    openCommand: (argv: readonly string[], label: string) => string | null,
  ): Promise<string> {
    const argv = await api.handoff(sessionId);
    // Run as is, without a shell: no quoting to get wrong, and the TUI exiting closes the tab.
    const ptyKey = openCommand(argv, "Terminal");
    if (!ptyKey) {
      await api.handback(sessionId);
      throw new Error("There is no pane to open the terminal in.");
    }
    tabs.set(ptyKey, { sessionId, openedAt: now() });
    return ptyKey;
  }

  return {
    tabFor,

    /** Whether the tab continues a provider session. */
    isHandoffTab: (ptyKey: string): boolean => tabs.has(ptyKey),

    start(
      sessionId: string,
      openCommand: (argv: readonly string[], label: string) => string | null,
    ): Promise<string> {
      const existing = tabFor(sessionId);
      if (existing) return Promise.resolve(existing);
      let pending = starting.get(sessionId);
      if (!pending) {
        pending = openTab(sessionId, openCommand).finally(() => starting.delete(sessionId));
        starting.set(sessionId, pending);
      }
      return pending;
    },

    /**
     * Return the session to its chat by closing its terminal, which hands it back through
     * shellClosed; without a terminal, hand it back directly.
     */
    async end(sessionId: string, closeTab: (ptyKey: string) => Promise<TabClose>): Promise<void> {
      // A handback sent before the tab opens would leave both the chat and the terminal driving.
      await starting.get(sessionId)?.catch(() => {});
      const ptyKey = tabFor(sessionId);
      if (!ptyKey) return api.handback(sessionId);
      const closed = await closeTab(ptyKey);
      if (closed === "starting") throw new Error("The terminal is still starting.");
      // Its terminal still runs, in a workspace not shown or not loaded yet.
      if (closed === "missing") {
        throw new Error("Close the session's terminal to return to the chat.");
      }
    },

    /**
     * Every shell tab that leaves the layout, closed or exited by itself, passes through
     * here; a handoff tab returns its session to the chat. A failed handback never fails the close.
     */
    async shellClosed(ptyKey: string, exited: boolean): Promise<void> {
      const tab = tabs.get(ptyKey);
      if (!tab) return;
      tabs.delete(ptyKey);
      try {
        await api.handback(tab.sessionId);
      } catch (error) {
        notify(
          `The session could not return to the chat: ${String(error)}. Try again from the session's chat.`,
        );
        return;
      }
      if (exited && now() - tab.openedAt < QUICK_EXIT_MS) {
        notify(
          "The terminal closed right after it opened, so the session is back in its chat. Check that the agent's CLI starts in a terminal.",
        );
      }
    },

    /** The session left the app: its terminal must not keep driving a conversation no one sees. */
    async release(sessionId: string): Promise<void> {
      // A handoff still asking the provider would otherwise open its terminal afterwards.
      await starting.get(sessionId)?.catch(() => {});
      const ptyKey = tabFor(sessionId);
      if (!ptyKey) return;
      // Forgotten first, so closing it does not hand back a session that has left.
      tabs.delete(ptyKey);
      if (!(await discardTab(ptyKey))) await closeTerminal(ptyKey);
    },
  };
}
