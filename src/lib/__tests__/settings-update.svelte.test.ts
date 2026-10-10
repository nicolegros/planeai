import { beforeEach, describe, expect, it, vi } from "vitest";

const update = vi.fn();
const get = vi.fn();
vi.mock("../api", () => ({
  config: { update: (...args: unknown[]) => update(...args), get: () => get() },
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
const loadTheme = vi.fn();
vi.mock("../theme-loader", () => ({ loadTheme: () => loadTheme() }));

const { getSettings, updateSettings } = await import("../settings.svelte");

/** Stands in for the backend: the last accepted config. */
let saved: ReturnType<typeof getSettings>;

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  saved = structuredClone($state.snapshot(getSettings()));
  update.mockReset();
  get.mockReset();
  loadTheme.mockReset();
  get.mockImplementation(async () => structuredClone(saved));
});

describe("updateSettings", () => {
  it("shows what the backend holds when it rejects the update", async () => {
    update.mockRejectedValueOnce("Invalid config");
    const before = getSettings().terminal.font_size;

    await expect(
      updateSettings({ terminal: { ...getSettings().terminal, font_size: 99 } }),
    ).rejects.toBe("Invalid config");

    expect(getSettings().terminal.font_size).toBe(before);
  });

  it("keeps the new value once the backend accepts it", async () => {
    update.mockResolvedValueOnce(undefined);

    await updateSettings({ vim_mode: false });

    expect(getSettings().vim_mode).toBe(false);
  });

  it("does not resurrect a stale value when overlapping updates both fail", async () => {
    const start = getSettings().terminal.font_size;
    const first = deferred();
    const second = deferred();
    update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const a = updateSettings({ terminal: { ...getSettings().terminal, font_size: start + 1 } });
    const b = updateSettings({ terminal: { ...getSettings().terminal, font_size: start + 2 } });
    first.reject("Invalid config");
    second.reject("Invalid config");
    await Promise.allSettled([a, b]);

    expect(getSettings().terminal.font_size).toBe(start);
  });

  it("keeps both changes when an earlier update fails but a later one saves them", async () => {
    const first = deferred();
    const second = deferred();
    update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const a = updateSettings({ sound_enabled: false });
    const b = updateSettings({ web_links: false });
    first.reject("Disk busy");
    // The later update carries the whole config, including the first change.
    saved = structuredClone($state.snapshot(getSettings()));
    second.resolve();
    await Promise.allSettled([a, b]);

    expect(getSettings().sound_enabled).toBe(false);
    expect(getSettings().web_links).toBe(false);
  });

  it("loads a theme saved by a later update even though its own update failed", async () => {
    const first = deferred();
    const second = deferred();
    update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const theme = getSettings().appearance.theme === "nord" ? "solarized" : "nord";

    const a = updateSettings({ appearance: { ...getSettings().appearance, theme } });
    const b = updateSettings({ vim_mode: true });
    first.reject("Disk busy");
    saved = structuredClone($state.snapshot(getSettings()));
    second.resolve();
    await Promise.allSettled([a, b]);

    expect(getSettings().appearance.theme).toBe(theme);
    expect(loadTheme).toHaveBeenCalledOnce();
  });

  it("does not reload the theme when it did not change", async () => {
    update.mockResolvedValueOnce(undefined);

    await updateSettings({ sound_enabled: true });

    expect(loadTheme).not.toHaveBeenCalled();
  });

  it("reloads a scheme pair theme only when its value changes", async () => {
    update.mockResolvedValue(undefined);
    const appearance = getSettings().appearance;

    await updateSettings({ appearance: { ...appearance, theme: { light: "A", dark: "B" } } });
    await updateSettings({ appearance: { ...appearance, theme: { light: "A", dark: "B" } } });
    expect(loadTheme).toHaveBeenCalledOnce();

    await updateSettings({ appearance: { ...appearance, theme: { light: "A", dark: "C" } } });
    expect(loadTheme).toHaveBeenCalledTimes(2);
  });
});
