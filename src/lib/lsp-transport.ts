import { Channel } from "@tauri-apps/api/core";
import type { Transport } from "@codemirror/lsp-client";
import { lsp, type LspConnection } from "./api";

/** Bridges CodeMirror's JSON-only LSP transport to the Rust stdio manager. */
export class TauriLspTransport implements Transport {
  private handlers = new Set<(value: string) => void>();
  private connection: LspConnection | null = null;

  private constructor() {}

  static async connect(repoPath: string, filePath: string): Promise<TauriLspTransport> {
    const transport = new TauriLspTransport();
    const onMessage = new Channel<string>();
    onMessage.onmessage = (message) => {
      for (const handler of transport.handlers) handler(message);
    };
    transport.connection = await lsp.connect(repoPath, filePath, onMessage);
    return transport;
  }

  get languageId(): string {
    if (!this.connection) throw new Error("LSP transport is not connected");
    return this.connection.language_id;
  }

  send(message: string): void {
    if (!this.connection) throw new Error("LSP transport is not connected");
    void lsp.send(this.connection.connection_id, message).catch((error) => {
      console.error("Failed to send LSP message:", error);
    });
  }

  subscribe(handler: (value: string) => void): void {
    this.handlers.add(handler);
  }

  unsubscribe(handler: (value: string) => void): void {
    this.handlers.delete(handler);
  }

  async close(): Promise<void> {
    const connection = this.connection;
    this.connection = null;
    this.handlers.clear();
    if (connection) await lsp.disconnect(connection.connection_id);
  }
}

export function fileUri(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  return `file://${encodeURI(normalized)}`;
}
