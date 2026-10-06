import { errorMessage } from "./errors";
import { parsePtyKey } from "./pty-key";
import type { TabClose } from "./task-workspace-layout.svelte";
import type { TabEndReason, TabSpec } from "./types";

/** A terminal that exits sooner than this after opening most likely failed to start the TUI. */
const QUICK_EXIT_MS = 5_000;

/** Opens a terminal tab running `spec` in the focused pane; null when there is no pane. */
type OpenCommand = (spec: TabSpec, label: string) => Promise<string | null>;

export interface ProviderHandoffDeps {
  api: {
    handoff(sessionId: string): Promise<string[]>;
    handback(sessionId: string): Promise<void>;
  };
  /** Closes a terminal's tab in the shown layout; false when that layout does not hold it. */
  discardTab(ptyKey: string): Promise<boolean>;
  /** Ends a terminal's process when its tab is in a layout not loaded. */
  closeTerminal(ptyKey: string): Promise<void>;
  /** Whether the terminal still runs its program: an app restart leaves a plain shell. */
  isProgramRunning(ptyKey: string): Promise<boolean>;
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
  isProgramRunning,
  notify,
  now = Date.now,
}: ProviderHandoffDeps) {
  /** Terminal tabs continuing a provider session, keyed by pty key. One per session. */
  const tabs = new Map<string, { sessionId: string; openedAt: number }>();
  /** Handoffs still asking the provider for its command, so a repeat request joins them. */
  const starting = new Map<string, Promise<string>>();
  /** Restored tabs being checked for a running program; a close or release meanwhile cancels. */
  const adopting = new Set<string>();
  /** Handbacks under way, by the pty key of the terminal that ended, so `end` can await them. */
  const handingBack = new Map<string, Promise<void>>();
  /** Terminals `end` is closing; it reports their handback's failure itself. */
  const ending = new Set<string>();

  /** Return a handoff tab's session to its chat, or join the handback its end already started. */
  function handBack(ptyKey: string): Promise<void> | undefined {
    const tab = tabs.get(ptyKey);
    if (!tab) return handingBack.get(ptyKey);
    tabs.delete(ptyKey);
    const pending = api.handback(tab.sessionId);
    handingBack.set(ptyKey, pending);
    // A failed one stays, so an `end` joining it late still fails.
    pending.then(
      () => handingBack.delete(ptyKey),
      () => {},
    );
    return pending;
  }

  function tabFor(sessionId: string): string | undefined {
    for (const [ptyKey, tab] of tabs) if (tab.sessionId === sessionId) return ptyKey;
    return undefined;
  }

  async function openTab(sessionId: string, openCommand: OpenCommand): Promise<string> {
    const argv = await api.handoff(sessionId);
    // Run as is, without a shell: no quoting to get wrong, and the TUI exiting closes the tab.
    let ptyKey: string | null;
    try {
      ptyKey = await openCommand({ kind: "program", argv }, "Terminal");
    } catch (error) {
      await api.handback(sessionId).catch((handbackError) => {
        console.warn("Failed to return the session to its chat", sessionId, handbackError);
      });
      throw error;
    }
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

    start(sessionId: string, openCommand: OpenCommand): Promise<string> {
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
     * tabEnded; without a terminal, hand it back directly.
     */
    async end(sessionId: string, closeTab: (ptyKey: string) => Promise<TabClose>): Promise<void> {
      // A handback sent before the tab opens would leave both the chat and the terminal driving.
      await starting.get(sessionId)?.catch(() => {});
      const ptyKey = tabFor(sessionId);
      if (!ptyKey) return api.handback(sessionId);
      ending.add(ptyKey);
      try {
        const closed = await closeTab(ptyKey);
        // Its terminal still runs, in a workspace not shown or not loaded yet.
        if (closed === "missing") {
          throw new Error("Close the session's terminal to return to the chat.");
        }
        // Its `tab-ended` may come after the close resolves: back in its chat only once handed back.
        await handBack(ptyKey);
      } finally {
        ending.delete(ptyKey);
      }
    },

    /**
     * Every terminal tab that ends passes through here once; a handoff tab returns its session
     * to the chat, unless the session itself ended. True when it was a handoff tab, whose end
     * this explains. A failed handback never fails the close.
     */
    async tabEnded(ptyKey: string, reason: TabEndReason, error?: string): Promise<boolean> {
      adopting.delete(ptyKey);
      const tab = tabs.get(ptyKey);
      if (!tab) return false;
      if (reason === "session_ended") {
        tabs.delete(ptyKey);
        return true;
      }
      try {
        await handBack(ptyKey);
      } catch (handbackError) {
        if (ending.has(ptyKey)) return true;
        notify(
          `The session could not return to the chat: ${errorMessage(handbackError)}. Try again from the session's chat.`,
        );
        return true;
      }
      if (reason === "failed_to_start") {
        notify(
          `The agent's terminal could not start${error ? `: ${error}` : ""}. The session is back in its chat.`,
        );
      } else if (reason === "exited" && now() - tab.openedAt < QUICK_EXIT_MS) {
        notify(
          "The terminal closed right after it opened, so the session is back in its chat. Check that the agent's CLI starts in a terminal.",
        );
      }
      return true;
    },

    /**
     * A loaded layout holds this handoff terminal, as after a webview reload: closing it must
     * still return its session to the chat. After an app restart its TUI is gone and the tab
     * is a plain shell, which must never receive the session's prompts.
     */
    async adopt(ptyKey: string): Promise<void> {
      const parts = parsePtyKey(ptyKey);
      if (parts?.kind !== "shell" || tabs.has(ptyKey) || adopting.has(ptyKey)) return;
      adopting.add(ptyKey);
      let running = false;
      try {
        running = await isProgramRunning(ptyKey);
      } finally {
        // Cancelled when its tab closed or its session left while this checked.
        if (!adopting.delete(ptyKey)) running = false;
      }
      if (!running || tabs.has(ptyKey)) return;
      // Not just opened: its exit says nothing about whether the TUI could start.
      tabs.set(ptyKey, { sessionId: parts.sessionId, openedAt: -Infinity });
    },

    /** The session left the app: its terminal must not keep driving a conversation no one sees. */
    async release(sessionId: string): Promise<void> {
      // A handoff still asking the provider would otherwise open its terminal afterwards.
      await starting.get(sessionId)?.catch(() => {});
      for (const ptyKey of adopting) {
        if (parsePtyKey(ptyKey)?.sessionId === sessionId) adopting.delete(ptyKey);
      }
      const ptyKey = tabFor(sessionId);
      if (!ptyKey) return;
      // Forgotten first, so closing it does not hand back a session that has left.
      tabs.delete(ptyKey);
      // The backend ends it too for a session archived, destroyed or exited, but not parked,
      // nor for a terminal that opened after the session left.
      if (!(await discardTab(ptyKey))) await closeTerminal(ptyKey);
    },
  };
}
