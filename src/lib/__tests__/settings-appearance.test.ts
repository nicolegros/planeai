import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({ config: {} }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("../theme-loader", () => ({ loadTheme: vi.fn() }));

describe("system appearance", () => {
  const originalMatchMedia = window.matchMedia;

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    document.documentElement.classList.remove("dark");
    document.documentElement.style.colorScheme = "";
    vi.resetModules();
  });

  it("notifies theme consumers after the OS appearance changes", async () => {
    let onChange: ((event: { matches: boolean }) => void) | undefined;
    window.matchMedia = ((query: string) => ({
      matches: false,
      media: query,
      addEventListener: (_: string, listener: (event: { matches: boolean }) => void) => {
        onChange = listener;
      },
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
    await import("../settings.svelte");

    const seen: boolean[] = [];
    const listener = () => seen.push(document.documentElement.classList.contains("dark"));
    window.addEventListener("planeai-theme-changed", listener);
    onChange?.({ matches: true });
    window.removeEventListener("planeai-theme-changed", listener);

    expect(seen).toEqual([true]);
  });
});
