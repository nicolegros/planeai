import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  getPending: vi.fn(),
  getVersion: vi.fn(),
  install: vi.fn(),
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
    updater: {
      getVersion: mocks.getVersion,
      getPending: mocks.getPending,
      check: mocks.check,
      install: mocks.install,
    },
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

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import PreferencesPage from "../PreferencesPage.svelte";
import { _resetForTests } from "../../lib/updater.svelte";

function clickButton(label: string) {
  const button = Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!button) throw new Error(`Missing ${label} button`);
  button.click();
}

afterEach(() => {
  _resetForTests();
  mocks.check.mockReset();
  mocks.getPending.mockReset();
  mocks.getVersion.mockReset();
  mocks.install.mockReset();
  document.body.replaceChildren();
});

describe("PreferencesPage app updates", () => {
  it("checks for updates and installs a manually discovered release from More", async () => {
    mocks.getVersion.mockResolvedValue("1.80.0");
    mocks.getPending.mockResolvedValue(null);
    mocks.check.mockResolvedValue({ version: "1.81.0", body: null });
    mocks.install.mockResolvedValue(undefined);

    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target });
    await tick();
    await Promise.resolve();
    await tick();

    clickButton("More");
    await tick();
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain("PlaneAI v1.80.0");
    });

    clickButton("Check for updates");
    await Promise.resolve();
    await tick();
    expect(mocks.check).toHaveBeenCalledOnce();
    expect(document.body.textContent).toContain("Update available: v1.81.0");

    clickButton("Install & Restart");
    await Promise.resolve();
    await tick();
    expect(mocks.install).toHaveBeenCalledOnce();
    expect(document.body.textContent).toContain("Downloading and installing v1.81.0");

    unmount(component);
  });

  it("shows a launch-discovered update retained by the backend", async () => {
    mocks.getVersion.mockResolvedValue("1.80.0");
    mocks.getPending.mockResolvedValue({ version: "1.81.0", body: null });

    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target });
    await tick();
    await Promise.resolve();
    await tick();

    clickButton("More");
    await tick();

    await vi.waitFor(() => {
      expect(mocks.getPending).toHaveBeenCalledOnce();
      expect(document.body.textContent).toContain("Update available: v1.81.0");
    });
    expect(
      Array.from(document.querySelectorAll("button")).some(
        (button) => button.textContent?.trim() === "Install & Restart",
      ),
    ).toBe(true);

    unmount(component);
  });
});
