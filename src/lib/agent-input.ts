import { providerSessions, pty } from "./api";
import { isPluginSession } from "./plugin-providers";
import { providerHandoff } from "./task-workspace-layout.svelte";
import type { Session } from "./types";

/**
 * Submit a message to a session's agent: through its provider, or typed into the terminal
 * running it, which for a chat continued in a terminal is that terminal.
 */
export async function sendToAgent(
  session: Pick<Session, "id" | "backend">,
  text: string,
): Promise<void> {
  const handoffTab = providerHandoff.tabFor(session.id);
  if (isPluginSession(session) && !handoffTab) return providerSessions.send(session.id, text);
  // Typed only into the agent's TUI: a plain shell left in its place would run the text.
  if (handoffTab && !(await pty.isProgramRunning(handoffTab))) {
    throw new Error(
      "The session's terminal no longer runs its agent. Close it to return to the chat.",
    );
  }
  const ptyKey = handoffTab ?? session.id;
  const typed = await pty.write(ptyKey, Array.from(new TextEncoder().encode(text)));
  // Enter goes on its own, so a TUI that reads the text as a paste still submits it.
  if (!typed || !(await pty.write(ptyKey, [0x0d]))) {
    throw new Error("The agent's terminal is not attached. Try again once it is ready.");
  }
}
