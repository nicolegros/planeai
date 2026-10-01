import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const theme = vi.hoisted(() => ({ dark: false }));
vi.mock("../settings.svelte", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../settings.svelte")>()),
  isDark: () => theme.dark,
}));

import { createXtermSurface, waitForFont } from "../xterm-surface";

describe("waitForFont", () => {
  let originalFonts: FontFaceSet;

  beforeEach(() => {
    originalFonts = document.fonts;
  });

  afterEach(() => {
    Object.defineProperty(document, "fonts", { value: originalFonts, configurable: true });
    vi.useRealTimers();
  });

  function stubFonts(
    load: ReturnType<typeof vi.fn>,
    ready: Promise<unknown> = Promise.resolve(),
  ): void {
    Object.defineProperty(document, "fonts", { value: { load, ready }, configurable: true });
  }

  it("resolves when document.fonts.load succeeds", async () => {
    const load = vi.fn().mockResolvedValue([]);
    stubFonts(load);

    await expect(waitForFont("Menlo", 14)).resolves.toBeUndefined();
    expect(load).toHaveBeenCalledWith('14px "Menlo"');
  });

  it("falls back to document.fonts.ready when load throws", async () => {
    stubFonts(vi.fn().mockRejectedValue(new Error("not supported")));

    await expect(waitForFont("Unknown Font", 14)).resolves.toBeUndefined();
  });

  it("resolves after the timeout when the font load stalls", async () => {
    vi.useFakeTimers();
    stubFonts(vi.fn().mockReturnValue(new Promise(() => {})), new Promise(() => {}));

    const result = waitForFont("Stalled Font", 14);
    vi.advanceTimersByTime(3000);
    await expect(result).resolves.toBeUndefined();
  });
});

describe("color scheme reports (DEC mode 2031)", () => {
  const encoder = new TextEncoder();

  function setup() {
    const replies: string[] = [];
    const surface = createXtermSurface({
      onData: () => {},
      onUserBytes: () => {},
      onReply: (bytes) => replies.push(String.fromCharCode(...bytes)),
      onTitle: () => {},
    });
    const write = (data: string) =>
      new Promise<void>((resolve) => surface.write(encoder.encode(data), resolve));
    return { surface, replies, write };
  }

  beforeEach(() => {
    theme.dark = false;
  });

  it("reports mode 2031 as resettable before it is enabled, and set after", async () => {
    const { replies, write } = setup();
    await write("\x1b[?2031$p");
    await write("\x1b[?2031h");
    await write("\x1b[?2031$p");
    expect(replies).toEqual(["\x1b[?2031;2$y", "\x1b[?2031;1$y"]);
  });

  it("answers a color scheme query with the current scheme", async () => {
    const { replies, write } = setup();
    await write("\x1b[?996n");
    theme.dark = true;
    await write("\x1b[?996n");
    expect(replies).toEqual(["\x1b[?997;2n", "\x1b[?997;1n"]);
  });

  it("notifies subscribed apps when the scheme flips", async () => {
    const { surface, replies, write } = setup();
    await write("\x1b[?1004;2031h");
    theme.dark = true;
    surface.refreshAppearance();
    surface.refreshAppearance();
    theme.dark = false;
    surface.refreshAppearance();
    expect(replies).toEqual(["\x1b[?997;1n", "\x1b[?997;2n"]);
  });

  it("stays silent for apps that did not subscribe or unsubscribed", async () => {
    const { surface, replies, write } = setup();
    theme.dark = true;
    surface.refreshAppearance();
    await write("\x1b[?2031h\x1b[?2031l");
    theme.dark = false;
    surface.refreshAppearance();
    expect(replies).toEqual([]);
  });

  it("drops the subscription on terminal reset", async () => {
    const { surface, replies, write } = setup();
    await write("\x1b[?2031h");
    surface.reset();
    await write("\x1b[?2031h\x1bc");
    theme.dark = true;
    surface.refreshAppearance();
    expect(replies).toEqual([]);
  });
});
