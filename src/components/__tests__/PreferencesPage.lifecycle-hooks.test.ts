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
    task_management: {
      on_start: { move_to: "in_progress" },
      on_complete: { move_to: "done" },
    },
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
  return Array.from(document.querySelectorAll("button")).find((candidate) =>
    candidate.textContent?.replace(/\s+/g, " ").trim().includes(label),
  );
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("PreferencesPage lifecycle hooks", () => {
  it("shows an unset hook as disabled instead of its suggested status", async () => {
    const host = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target: host });
    await tick();
    findButton("Task Management")!.click();
    await tick();

    const hookInput = (label: string) =>
      Array.from(host.querySelectorAll("label"))
        .find((el) => el.textContent?.startsWith(label))!
        .parentElement!.querySelector("input")!;

    expect(hookInput("On start").value).toBe("in_progress");
    expect(hookInput("On notify").value).toBe("");
    expect(hookInput("On notify").placeholder).toBe("Disabled - e.g. in_review");
    unmount(component);
  });
});
