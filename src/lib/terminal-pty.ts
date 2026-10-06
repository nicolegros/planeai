import { Channel } from "@tauri-apps/api/core";
import { pty } from "./api";
import { parsePtyKey } from "./pty-key";
import { isDark } from "./settings.svelte";
import type { TerminalPty } from "./terminal-view";

/** A terminal tab that ended instead of attaching; its `tab-ended` reports why, if it was news. */
export class TabEndedError extends Error {
  constructor(readonly ptyKey: string) {
    super(`${ptyKey} has ended`);
  }
}

/** Production PTY adapter over the typed api layer. */
export const tauriTerminalPty: TerminalPty = {
  async connect({ ptyKey, kind }, onData) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (raw) => onData(new Uint8Array(raw));
    if (kind === "agent") {
      await pty.attach(ptyKey, isDark(), channel);
      return;
    }
    const parts = parsePtyKey(ptyKey);
    if (parts?.kind !== "shell") throw new Error(`Not a shell tab: ${ptyKey}`);
    if (!(await pty.attachTab(parts.sessionId, parts.index, isDark(), channel))) {
      throw new TabEndedError(ptyKey);
    }
  },
  write: (ptyKey, bytes) => {
    void pty.write(ptyKey, bytes);
  },
  resize: (ptyKey, { cols, rows }) => {
    void pty.resize(ptyKey, rows, cols);
  },
  pause: (ptyKey) => {
    void pty.pause(ptyKey);
  },
  resume: (ptyKey) => {
    void pty.resume(ptyKey);
  },
};
