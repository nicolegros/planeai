import { afterEach, describe, expect, it, vi } from "vitest";

const { configGet } = vi.hoisted(() => ({ configGet: vi.fn() }));

vi.mock("../api", () => ({ config: { get: configGet } }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("../theme-loader", () => ({
  loadTheme: vi.fn(),
  THEME_CHANGED_EVENT: "planeai-theme-changed",
}));

describe("appearance", () => {
  const originalMatchMedia = window.matchMedia;

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    document.documentElement.classList.remove("dark");
    document.documentElement.style.colorScheme = "";
    vi.resetModules();
  });

  async function loadWithSystemDark(initiallyDark: boolean) {
    let onChange: ((event: { matches: boolean }) => void) | undefined;
    window.matchMedia = ((query: string) => ({
      matches: initiallyDark,
      media: query,
      addEventListener: (_: string, listener: (event: { matches: boolean }) => void) => {
        onChange = listener;
      },
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
    const settings = await import("../settings.svelte");
    return { settings, setSystemDark: (matches: boolean) => onChange?.({ matches }) };
  }

  it("resolves the system preference to a light or dark mode", async () => {
    const { settings, setSystemDark } = await loadWithSystemDark(true);
    expect(settings.getAppearance()).toEqual({ mode: "dark", preference: "system" });
    setSystemDark(false);
    expect(settings.getAppearance()).toEqual({ mode: "light", preference: "system" });
  });

  it("notifies theme consumers after the OS appearance changes in system mode", async () => {
    const { setSystemDark } = await loadWithSystemDark(false);
    const seen: boolean[] = [];
    const listener = () => seen.push(document.documentElement.classList.contains("dark"));
    window.addEventListener("planeai-theme-changed", listener);
    setSystemDark(true);
    window.removeEventListener("planeai-theme-changed", listener);
    expect(seen).toEqual([true]);
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("notifies theme consumers when an explicit mode is loaded", async () => {
    const { settings } = await loadWithSystemDark(false);
    configGet.mockResolvedValue({ appearance: { mode: "dark", theme: "default" } });
    const listener = vi.fn(() =>
      expect(document.documentElement.classList.contains("dark")).toBe(true),
    );
    window.addEventListener("planeai-theme-changed", listener);
    await settings.loadSettings();
    window.removeEventListener("planeai-theme-changed", listener);
    expect(listener).toHaveBeenCalledOnce();
    expect(settings.getAppearance()).toEqual({ mode: "dark", preference: "dark" });
  });
});
