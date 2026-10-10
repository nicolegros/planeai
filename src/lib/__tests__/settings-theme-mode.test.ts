import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({ config: {}, preferences: {} }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));

describe("theme CSS per appearance mode", () => {
  const originalMatchMedia = window.matchMedia;

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    document.documentElement.classList.remove("dark");
    document.head.innerHTML = "";
    vi.resetModules();
  });

  it("applies the dark CSS before notifying theme consumers of an OS appearance change", async () => {
    let onChange: ((event: { matches: boolean }) => void) | undefined;
    window.matchMedia = ((query: string) => ({
      matches: false,
      media: query,
      addEventListener: (_: string, listener: (event: { matches: boolean }) => void) => {
        onChange = listener;
      },
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
    const { injectTheme } = await import("../theme-loader");
    await import("../settings.svelte");
    injectTheme({
      light: ":root { --terminal-background: #fafafa; }",
      dark: ":root, .dark { --terminal-background: #282a36; }",
    });

    const seen: string[] = [];
    const listener = () =>
      seen.push(
        getComputedStyle(document.documentElement).getPropertyValue("--terminal-background").trim(),
      );
    window.addEventListener("planeai-theme-changed", listener);
    onChange?.({ matches: true });
    window.removeEventListener("planeai-theme-changed", listener);

    expect(seen).toEqual(["#282a36"]);
  });
});
