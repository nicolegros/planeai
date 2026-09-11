import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  config: {
    appearance: { mode: "system" as const, theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } },
    default_provider: "kiro",
  },
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    preferences: {
      ...actual.preferences,
      listMonospaceFonts: vi.fn(() => Promise.resolve([])),
      listThemes: vi.fn(() => Promise.resolve([])),
      checkTmuxAvailable: vi.fn(() => Promise.resolve(true)),
      checkCliInstalled: vi.fn(() => Promise.resolve(true)),
      listStaleWorktrees: vi.fn(() => Promise.resolve([])),
      runStaleWorktreeCleanup: vi.fn(() => Promise.resolve([])),
      installCli: vi.fn(),
      getLogDir: vi.fn(() => Promise.resolve("/tmp")),
    },
    plugins: { ...actual.plugins, list: vi.fn(() => Promise.resolve([])) },
  };
});
vi.mock("../../lib/settings.svelte", () => ({
  loadSettings: vi.fn(),
  getSettings: () => mocks.config,
  updateSettings: mocks.updateSettings,
  refreshSettings: vi.fn(),
}));
vi.mock("../../lib/theme-loader", () => ({ loadTheme: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));

import PreferencesPage from "../PreferencesPage.svelte";

function clickButton(label: string) {
  const button = Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!button) throw new Error(`Missing ${label} button`);
  button.click();
}

afterEach(() => {
  mocks.updateSettings.mockClear();
  document.body.replaceChildren();
});

describe("PreferencesPage editor", () => {
  it("saves an external editor preset as typed editor settings", async () => {
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target });
    await tick();

    clickButton("Editor");
    await tick();
    clickButton("External app");
    await tick();
    clickButton("VS Code");
    await tick();
    clickButton("Save editor settings");
    await tick();

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      editor: {
        mode: "external",
        command: "code",
        args: ["--reuse-window", "--goto", "{file}"],
      },
    });
    unmount(component);
  });

  it("does not save an incomplete terminal editor configuration", async () => {
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target });
    await tick();

    clickButton("Editor");
    await tick();
    clickButton("Terminal command");
    await tick();
    clickButton("Save editor settings");
    await tick();

    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Editor executable is required.");
    unmount(component);
  });
});
