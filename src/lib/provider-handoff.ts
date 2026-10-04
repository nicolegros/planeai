import { providerSessions } from "./api";
import { showSnackbar } from "./snackbar.svelte";
import type { TabClose } from "./task-workspace-layout.svelte";

export interface ProviderHandoffDeps {
  api: {
    handoff(sessionId: string): Promise<string[]>;
    handback(sessionId: string): Promise<void>;
  };
  notify(message: string): void;
}

/**
 * Continues provider sessions in their agent's TUI. The provider detaches first, so
 * only one side ever drives the conversation; closing the tab hands it back.
 */
export function createProviderHandoff({ api, notify }: ProviderHandoffDeps) {
  /** Terminal tabs continuing a provider session, keyed by pty key. One per session. */
  const tabs = new Map<string, string>();
  /** Handoffs still asking the provider for its command, so a repeat request joins them. */
  const starting = new Map<string, Promise<string>>();

  function tabFor(sessionId: string): string | undefined {
    for (const [ptyKey, owner] of tabs) if (owner === sessionId) return ptyKey;
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
    tabs.set(ptyKey, sessionId);
    return ptyKey;
  }

  return {
    tabFor,

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
      // Its shell still runs, in a workspace not shown or not loaded yet.
      if (closed === "missing") {
        throw new Error("Close the session's terminal to return to the chat.");
      }
    },

    /**
     * Every shell tab that leaves the layout, closed or exited by itself, passes through
     * here; a handoff tab returns its session to the chat. A failed handback never fails the close.
     */
    async shellClosed(ptyKey: string): Promise<void> {
      const sessionId = tabs.get(ptyKey);
      if (!sessionId) return;
      tabs.delete(ptyKey);
      try {
        await api.handback(sessionId);
      } catch (error) {
        notify(
          `The session could not return to the chat: ${String(error)}. Try again from the session's chat.`,
        );
      }
    },
  };
}

// Looked up per call, so importing this module never touches the API.
export const providerHandoff = createProviderHandoff({
  api: {
    handoff: (sessionId) => providerSessions.handoff(sessionId),
    handback: (sessionId) => providerSessions.handback(sessionId),
  },
  notify: (message) => showSnackbar(message),
});
