import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  checkRmuxAvailable: vi.fn(() => Promise.resolve(true)),
  config: {
    appearance: { mode: "system" as const, theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } },
    default_provider: "kiro",
    session_backend: null as string | null,
  },
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    updater: {
      getVersion: vi.fn(() => Promise.resolve("0.0.0")),
      getPending: vi.fn(() => Promise.resolve(null)),
      check: vi.fn(),
      install: vi.fn(),
    },
    preferences: {
      ...actual.preferences,
      listMonospaceFonts: vi.fn(() => Promise.resolve([])),
      listThemes: vi.fn(() => Promise.resolve([])),
      checkTmuxAvailable: vi.fn(() => Promise.resolve(true)),
      checkRmuxAvailable: mocks.checkRmuxAvailable,
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
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import PreferencesPage from "../PreferencesPage.svelte";

function findButton(label: string) {
  return Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
}

/// The Session Backend section lives on the "More" tab, so every case has to
/// navigate there before asserting.
async function render() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const component = mount(PreferencesPage, { target: host });
  await tick();
  await Promise.resolve();
  await tick();

  const moreTab = findButton("More");
  if (!moreTab) throw new Error("Missing More tab");
  moreTab.click();
  await tick();
  await Promise.resolve();
  await tick();

  return { host, component };
}

afterEach(() => {
  mocks.updateSettings.mockClear();
  mocks.checkRmuxAvailable.mockClear();
  mocks.checkRmuxAvailable.mockImplementation(() => Promise.resolve(true));
  mocks.config.session_backend = null;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("session backend preferences", () => {
  it("offers rmux alongside the other backends", async () => {
    const { component } = await render();

    for (const label of ["Local", "tmux", "Daemon (experimental)", "rmux (experimental)"]) {
      expect(findButton(label), `missing ${label} option`).toBeTruthy();
    }

    unmount(component);
  });

  it("persists rmux as the session backend when selected", async () => {
    const { component } = await render();

    findButton("rmux (experimental)")!.click();
    await tick();

    expect(mocks.updateSettings).toHaveBeenCalledWith({ session_backend: "rmux" });

    unmount(component);
  });

  it("warns when rmux is selected but the binary is missing", async () => {
    mocks.checkRmuxAvailable.mockImplementation(() => Promise.resolve(false));
    mocks.config.session_backend = "rmux";

    const { host, component } = await render();
    await Promise.resolve();
    await tick();

    expect(host.textContent).toContain("rmux not found on PATH");

    unmount(component);
  });

  it("does not warn about rmux when the binary is present", async () => {
    mocks.config.session_backend = "rmux";

    const { host, component } = await render();
    await Promise.resolve();
    await tick();

    expect(host.textContent).not.toContain("rmux not found on PATH");
    // The description still tells the user the backend needs rmux.
    expect(host.textContent).toContain("requires rmux");

    unmount(component);
  });
});
