import { Channel } from "@tauri-apps/api/core";
import { pty } from "./api";
import { isDark } from "./settings.svelte";
import type { TerminalPty } from "./terminal-view";

/** Production PTY adapter over the typed api layer. */
export const tauriTerminalPty: TerminalPty = {
  async connect({ ptyKey, kind, initialCommand }, onData) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (raw) => onData(new Uint8Array(raw));
    if (kind === "agent") {
      await pty.attach(ptyKey, isDark(), channel);
      return;
    }
    // Shell pty keys are `<sessionId>:<tabIndex>`.
    const separator = ptyKey.lastIndexOf(":");
    const sessionId = ptyKey.slice(0, separator);
    const tabIndex = Number.parseInt(ptyKey.slice(separator + 1), 10);
    await pty.spawnTab(sessionId, tabIndex, isDark(), channel, initialCommand);
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
