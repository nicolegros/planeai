/**
 * Terminal views keyed by pty key, alive independently of the layout.
 *
 * Layout changes (TaskWorkspace switch, split, tab moves) only re-parent a
 * view; it keeps its buffer and PTY connection. A view is disposed only when
 * its tab is closed or its session leaves the app.
 */
import {
  createTerminalView,
  type GeometryHint,
  type TerminalView,
  type TerminalViewOptions,
} from "./terminal-view";
import { createXtermSurface } from "./xterm-surface";
import { tauriTerminalPty } from "./terminal-pty";

export type AcquireOptions = Omit<TerminalViewOptions, "pty" | "createSurface"> &
  Partial<Pick<TerminalViewOptions, "pty" | "createSurface">>;

const views = new Map<string, TerminalView>();
const geometryHint: GeometryHint = { last: null };

/** Return the live view for a pty key, creating it on first use. */
export function acquireTerminalView(options: AcquireOptions): TerminalView {
  const existing = views.get(options.ptyKey);
  if (existing) return existing;
  const view = createTerminalView({
    pty: tauriTerminalPty,
    createSurface: createXtermSurface,
    geometryHint,
    ...options,
  });
  views.set(options.ptyKey, view);
  return view;
}

export function disposeTerminalView(ptyKey: string): void {
  views.get(ptyKey)?.dispose();
  views.delete(ptyKey);
}

/** Dispose a session's agent view and all of its shell views (`<sessionId>:<n>`). */
export function disposeSessionTerminalViews(sessionId: string): void {
  for (const ptyKey of views.keys()) {
    if (ptyKey === sessionId || ptyKey.startsWith(`${sessionId}:`)) disposeTerminalView(ptyKey);
  }
}

export function disposeAllTerminalViews(): void {
  for (const ptyKey of views.keys()) disposeTerminalView(ptyKey);
}
