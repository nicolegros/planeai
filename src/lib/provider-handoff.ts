import { providerSessions } from "./api";
import { IS_WINDOWS } from "./keyboard";
import { showSnackbar } from "./snackbar.svelte";
import type { TabClose } from "./task-workspace-layout.svelte";

const POSIX_SAFE = /^[A-Za-z0-9_/.:=@%+,-]+$/;

/** Quote one argument for the shell a shell tab runs its command in. */
export function quoteArgument(argument: string, windows = IS_WINDOWS): string {
  if (windows) return /[\s"&|<>^]/.test(argument) ? `"${argument.replace(/"/g, '""')}"` : argument;
  return POSIX_SAFE.test(argument) ? argument : `'${argument.replace(/'/g, `'\\''`)}'`;
}

export function shellCommand(argv: string[], windows = IS_WINDOWS): string {
  return argv.map((argument) => quoteArgument(argument, windows)).join(" ");
}

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
    openCommand: (command: string, label: string) => string | null,
  ): Promise<string> {
    const argv = await api.handoff(sessionId);
    const ptyKey = openCommand(shellCommand(argv), "Terminal");
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
      openCommand: (command: string, label: string) => string | null,
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

    /** Return the session to its chat by closing its terminal, or directly when the tab is gone. */
    async end(sessionId: string, closeTab: (ptyKey: string) => Promise<TabClose>): Promise<void> {
      const ptyKey = tabFor(sessionId);
      const closed = ptyKey ? await closeTab(ptyKey) : "missing";
      if (closed === "starting") throw new Error("The terminal is still starting.");
      // Closing the tab hands the session back through shellClosed.
      if (closed === "missing") {
        if (ptyKey) tabs.delete(ptyKey);
        await api.handback(sessionId);
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

export type ProviderHandoff = ReturnType<typeof createProviderHandoff>;

// Looked up per call, so importing this module never touches the API.
export const providerHandoff = createProviderHandoff({
  api: {
    handoff: (sessionId) => providerSessions.handoff(sessionId),
    handback: (sessionId) => providerSessions.handback(sessionId),
  },
  notify: (message) => showSnackbar(message),
});
