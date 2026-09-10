import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const channels: MockChannel<string>[] = [];

  class MockChannel<T> {
    onmessage: ((value: T) => void) | null = null;

    constructor() {
      channels.push(this as unknown as MockChannel<string>);
    }
  }

  return {
    MockChannel,
    channels,
    connect: vi.fn(),
    send: vi.fn(),
    disconnect: vi.fn(),
  };
});

vi.mock("@tauri-apps/api/core", () => ({ Channel: mocks.MockChannel }));
vi.mock("../api", () => ({
  lsp: { connect: mocks.connect, send: mocks.send, disconnect: mocks.disconnect },
}));

import { fileUri, TauriLspTransport } from "../lsp-transport";

describe("TauriLspTransport", () => {
  beforeEach(() => {
    mocks.channels.length = 0;
    vi.clearAllMocks();
    mocks.connect.mockResolvedValue({
      connection_id: "lsp-1",
      language_id: "typescript",
      profile_id: "typescript-language-server",
    });
    mocks.send.mockResolvedValue(undefined);
    mocks.disconnect.mockResolvedValue(undefined);
  });

  it("connects through the typed API and relays server messages to subscribers", async () => {
    const transport = await TauriLspTransport.connect("/repo", "/repo/src/app.ts");
    const handler = vi.fn();
    transport.subscribe(handler);

    expect(mocks.connect).toHaveBeenCalledWith("/repo", "/repo/src/app.ts", mocks.channels[0]);
    mocks.channels[0].onmessage?.('{"method":"window/logMessage"}');
    expect(handler).toHaveBeenCalledWith('{"method":"window/logMessage"}');
    expect(transport.languageId).toBe("typescript");
  });

  it("forwards JSON messages and disconnects the backend connection", async () => {
    const transport = await TauriLspTransport.connect("/repo", "/repo/src/app.ts");
    transport.send('{"method":"initialized"}');
    await transport.close();

    expect(mocks.send).toHaveBeenCalledWith("lsp-1", '{"method":"initialized"}');
    expect(mocks.disconnect).toHaveBeenCalledWith("lsp-1");
  });

  it("produces a file URI accepted by the LSP client", () => {
    expect(fileUri("/repo/a file.ts")).toBe("file:///repo/a%20file.ts");
    expect(fileUri("C:\\repo\\main.rs")).toBe("file://C:/repo/main.rs");
  });
});
