import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  config: {
    appearance: { mode: "system" as const, theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } },
    default_provider: "kiro",
    sidebar_group_by: null as "project" | "status" | null,
    hide_task_keys: null as boolean | null,
    hide_project_labels: null as boolean | null,
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
      checkRmuxAvailable: vi.fn(() => Promise.resolve(true)),
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

// The Sidebar section lives on the default "Appearance" tab.
async function render() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const component = mount(PreferencesPage, { target: host });
  await tick();
  await Promise.resolve();
  await tick();
  return { host, component };
}

function checkbox(host: HTMLElement, label: string): HTMLInputElement {
  const match = [...host.querySelectorAll("label")].find((el) =>
    el.textContent?.trim().startsWith(label),
  );
  if (!match) throw new Error(`Missing ${label} checkbox`);
  return match.querySelector("input")!;
}

afterEach(() => {
  mocks.updateSettings.mockClear();
  mocks.config.sidebar_group_by = null;
  mocks.config.hide_task_keys = null;
  mocks.config.hide_project_labels = null;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("sidebar preferences", () => {
  it("defaults to project grouping with both labels shown and project labels disabled", async () => {
    const { host, component } = await render();

    const radios = [...host.querySelectorAll<HTMLInputElement>("input[name='sidebar-group-by']")];
    expect(radios.map((r) => [r.closest("label")?.textContent?.trim(), r.checked])).toEqual([
      ["Project", true],
      ["Status", false],
    ]);
    expect(checkbox(host, "Show task keys").checked).toBe(true);
    expect(checkbox(host, "Show project labels").checked).toBe(true);
    expect(checkbox(host, "Show project labels").disabled).toBe(true);

    unmount(component);
  });

  it("saves the grouping and label choices", async () => {
    mocks.config.sidebar_group_by = "status";
    const { host, component } = await render();

    host
      .querySelector<HTMLInputElement>("input[name='sidebar-group-by'][value='project']")!
      .click();
    checkbox(host, "Show task keys").click();
    expect(checkbox(host, "Show project labels").disabled).toBe(false);
    checkbox(host, "Show project labels").click();
    await tick();

    expect(mocks.updateSettings.mock.calls.map(([patch]) => patch)).toEqual([
      { sidebar_group_by: "project" },
      { hide_task_keys: true },
      { hide_project_labels: true },
    ]);

    unmount(component);
  });
});
