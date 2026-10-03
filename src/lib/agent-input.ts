import { providerSessions, pty } from "./api";
import { isPluginSession } from "./plugin-providers";
import type { Session } from "./types";

/** Submit a message to a session's agent: through its provider, or typed into its terminal. */
export async function sendToAgent(
  session: Pick<Session, "id" | "backend">,
  text: string,
): Promise<void> {
  if (isPluginSession(session)) return providerSessions.send(session.id, text);
  const typed = await pty.write(session.id, Array.from(new TextEncoder().encode(text)));
  // Enter goes on its own, so a TUI that reads the text as a paste still submits it.
  if (!typed || !(await pty.write(session.id, [0x0d]))) {
    throw new Error("The agent's terminal is not attached. Try again once it is ready.");
  }
}
