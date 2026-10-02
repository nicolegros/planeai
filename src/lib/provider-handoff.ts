import { providerSessions } from "./api";
import { IS_WINDOWS } from "./keyboard";

const POSIX_SAFE = /^[A-Za-z0-9_/.:=@%+,-]+$/;

/** Quote one argument for the shell a shell tab runs its command in. */
export function quoteArgument(argument: string, windows = IS_WINDOWS): string {
  if (windows) return /[\s"&|<>^]/.test(argument) ? `"${argument.replace(/"/g, '""')}"` : argument;
  return POSIX_SAFE.test(argument) ? argument : `'${argument.replace(/'/g, `'\\''`)}'`;
}

export function shellCommand(argv: string[], windows = IS_WINDOWS): string {
  return argv.map((argument) => quoteArgument(argument, windows)).join(" ");
}

/** Terminal tabs continuing a provider session, keyed by pty key. One per session. */
const handoffTabs = new Map<string, string>();
/** Handoffs still asking the provider for its command, so a repeat request joins them. */
const starting = new Map<string, Promise<string>>();

export function handoffTabFor(sessionId: string): string | undefined {
  for (const [ptyKey, owner] of handoffTabs) if (owner === sessionId) return ptyKey;
  return undefined;
}

/**
 * Continue a provider session in its agent's TUI. The provider detaches first, so
 * only one side ever drives the conversation; closing the tab hands it back.
 */
export function startHandoff(
  sessionId: string,
  openCommand: (command: string, label: string) => string | null,
): Promise<string> {
  const existing = handoffTabFor(sessionId);
  if (existing) return Promise.resolve(existing);
  let pending = starting.get(sessionId);
  if (!pending) {
    pending = openHandoffTab(sessionId, openCommand).finally(() => starting.delete(sessionId));
    starting.set(sessionId, pending);
  }
  return pending;
}

async function openHandoffTab(
  sessionId: string,
  openCommand: (command: string, label: string) => string | null,
): Promise<string> {
  const argv = await providerSessions.handoff(sessionId);
  const ptyKey = openCommand(shellCommand(argv), "Terminal");
  if (!ptyKey) {
    await providerSessions.handback(sessionId);
    throw new Error("There is no pane to open the terminal in.");
  }
  handoffTabs.set(ptyKey, sessionId);
  return ptyKey;
}

/** Every shell tab close passes through here; a handoff tab returns its session to the chat. */
export async function shellTabClosed(ptyKey: string): Promise<void> {
  const sessionId = handoffTabs.get(ptyKey);
  if (!sessionId) return;
  handoffTabs.delete(ptyKey);
  await providerSessions.handback(sessionId);
}
