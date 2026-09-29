import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { waitForFont } from "../xterm-surface";

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
