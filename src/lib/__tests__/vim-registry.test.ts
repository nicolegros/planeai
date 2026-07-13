import { describe, it, expect, vi, beforeEach } from "vitest";

// Capture the ex command handlers registered via Vim.defineEx
const exCommands: Record<string, Function> = {};

vi.mock("@replit/codemirror-vim", () => ({
  Vim: {
    defineEx: vi.fn((name: string, _prefix: string, fn: Function) => {
      exCommands[name] = fn;
    }),
  },
  getCM: vi.fn((view: any) => view._mockCm),
}));

import { registerEditor, unregisterEditor, type VimHandlers } from "../vim-registry";

function makeMockView(id: string) {
  const mockCm = {
    cm6: null as any,
    on: vi.fn(),
    off: vi.fn(),
  };
  const view = { _mockCm: mockCm, _id: id } as any;
  mockCm.cm6 = view;
  return { view, mockCm };
}

function makeHandlers(overrides: Partial<VimHandlers> = {}): VimHandlers {
  return {
    save: vi.fn(),
    close: vi.fn(),
    closeAll: vi.fn(),
    saveAndClose: vi.fn(),
    nextBuffer: vi.fn(),
    prevBuffer: vi.fn(),
    onModeChange: vi.fn(),
    ...overrides,
  };
}

describe("vim-registry", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("routes :w to the correct editor based on cm.cm6", () => {
    const { view: viewA, mockCm: cmA } = makeMockView("A");
    const { view: viewB, mockCm: cmB } = makeMockView("B");
    const handlersA = makeHandlers();
    const handlersB = makeHandlers();

    registerEditor(viewA, handlersA);
    registerEditor(viewB, handlersB);

    // Simulate :w in editor A
    exCommands["w"](cmA, {});
    expect(handlersA.save).toHaveBeenCalledTimes(1);
    expect(handlersB.save).not.toHaveBeenCalled();

    // Simulate :w in editor B
    exCommands["w"](cmB, {});
    expect(handlersB.save).toHaveBeenCalledTimes(1);
    expect(handlersA.save).toHaveBeenCalledTimes(1); // still 1
  });

  it("routes :q and :q! to the correct editor", () => {
    const { view: viewA, mockCm: cmA } = makeMockView("A");
    const { view: viewB, mockCm: cmB } = makeMockView("B");
    const handlersA = makeHandlers();
    const handlersB = makeHandlers();

    registerEditor(viewA, handlersA);
    registerEditor(viewB, handlersB);

    exCommands["q"](cmA, {});
    expect(handlersA.close).toHaveBeenCalledWith(false);

    exCommands["q"](cmB, { bang: true });
    expect(handlersB.close).toHaveBeenCalledWith(true);
    expect(handlersA.close).toHaveBeenCalledTimes(1);
  });

  it("routes :wq to the correct editor", () => {
    const { view: viewA, mockCm: _cmA } = makeMockView("A");
    const { view: viewB, mockCm: cmB } = makeMockView("B");
    const handlersA = makeHandlers();
    const handlersB = makeHandlers();

    registerEditor(viewA, handlersA);
    registerEditor(viewB, handlersB);

    exCommands["wq"](cmB, {});
    expect(handlersB.saveAndClose).toHaveBeenCalledTimes(1);
    expect(handlersA.saveAndClose).not.toHaveBeenCalled();
  });

  it("unregister removes the editor from routing", () => {
    const { view: viewA, mockCm: cmA } = makeMockView("A");
    const handlersA = makeHandlers();

    registerEditor(viewA, handlersA);
    unregisterEditor(viewA);

    exCommands["w"](cmA, {});
    expect(handlersA.save).not.toHaveBeenCalled();
  });

  it("registers vim-mode-change listener on cm adapter", () => {
    const { view: viewA, mockCm: cmA } = makeMockView("A");
    const handlersA = makeHandlers();

    registerEditor(viewA, handlersA);

    expect(cmA.on).toHaveBeenCalledWith("vim-mode-change", expect.any(Function));
  });

  it("unregisters vim-mode-change listener on cleanup", () => {
    const { view: viewA, mockCm: cmA } = makeMockView("A");
    const handlersA = makeHandlers();

    registerEditor(viewA, handlersA);
    const listener = cmA.on.mock.calls[0][1];

    unregisterEditor(viewA);
    expect(cmA.off).toHaveBeenCalledWith("vim-mode-change", listener);
  });
});
