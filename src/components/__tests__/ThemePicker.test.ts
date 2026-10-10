import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";
import type { AppConfig } from "../../lib/settings.svelte";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  config: {} as AppConfig,
  updateSettings: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mocks.invoke(...args) }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => mocks.config,
  updateSettings: mocks.updateSettings,
}));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

const { default: ThemePicker } = await import("../settings/ThemePicker.svelte");

const SCHEMES = [
  { name: "Dracula", dark: true, background: "#282a36", foreground: "#f8f8f2", accent: "#bd93f9" },
  {
    name: "One Half Dark",
    dark: true,
    background: "#282c34",
    foreground: "#dcdfe4",
    accent: "#61afef",
  },
  {
    name: "One Half Light",
    dark: false,
    background: "#fafafa",
    foreground: "#383a42",
    accent: "#0184bc",
  },
];

async function flush() {
  for (let i = 0; i < 3; i++) await tick();
}

describe("ThemePicker", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    mocks.config = {
      appearance: { mode: "system", theme: "one" },
      terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
      providers: {},
      default_provider: "kiro",
    };
    mocks.updateSettings.mockResolvedValue(undefined);
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "list_themes") return Promise.resolve(["default", "one"]);
      if (command === "list_terminal_schemes") return Promise.resolve(SCHEMES);
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
    vi.clearAllMocks();
  });

  it("checks the CSS theme that is selected", async () => {
    component = mount(ThemePicker, { target });
    await flush();

    const checked = [...target.querySelectorAll('[role="radio"][aria-checked="true"]')].map(
      (el) => el.textContent,
    );
    expect(checked).toEqual(["one"]);
  });

  it("shows the active CSS theme in both mode selects", async () => {
    component = mount(ThemePicker, { target });
    await flush();

    const values = ["Light", "Dark"].map(
      (side) =>
        target.querySelector<HTMLInputElement>(`input[aria-label="${side} color scheme"]`)!.value,
    );
    expect(values).toEqual(["One", "One"]);
  });

  it("keeps the light CSS theme when a dark scheme is picked", async () => {
    component = mount(ThemePicker, { target });
    await flush();

    const input = target.querySelector<HTMLInputElement>('input[aria-label="Dark color scheme"]')!;
    input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    flushSync();
    await tick();
    input.value = "half";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      appearance: { mode: "system", theme: { light: { css: "one" }, dark: "One Half Dark" } },
    });
  });

  it("lists CSS themes, then only light schemes, in the light select", async () => {
    component = mount(ThemePicker, { target });
    await flush();

    const input = target.querySelector<HTMLInputElement>('input[aria-label="Light color scheme"]')!;
    input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    await flush();

    const options = [...document.querySelectorAll("[data-combobox-item]")].map((el) => ({
      label: el.querySelector("span")?.textContent,
      badge: el.querySelectorAll("span")[1]?.textContent ?? null,
    }));
    expect(options).toEqual([
      { label: "Default", badge: "planeai" },
      { label: "One", badge: "planeai" },
      { label: "One Half Light", badge: null },
    ]);
  });
});
