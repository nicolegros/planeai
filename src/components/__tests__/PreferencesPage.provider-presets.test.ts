import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  config: {
    appearance: { mode: "system" as const, theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } } as Record<string, unknown>,
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

function findButton(label: string) {
  return Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
}

async function render() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const component = mount(PreferencesPage, { target: host });
  await tick();
  await Promise.resolve();
  await tick();
  findButton("Models")!.click();
  await tick();
  return { host, component };
}

afterEach(() => {
  mocks.updateSettings.mockClear();
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("provider presets", () => {
  it("only offers presets for agents that are not configured yet", async () => {
    const { component } = await render();

    expect(findButton("+ Kiro")).toBeUndefined();
    for (const label of ["+ Claude Code", "+ Copilot", "+ Codex"]) {
      expect(findButton(label), `missing ${label}`).toBeTruthy();
    }

    unmount(component);
  });

  it("adds the full Codex provider entry in one click", async () => {
    const { component } = await render();

    findButton("+ Codex")!.click();
    await tick();

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      providers: {
        kiro: { command: "kiro-cli chat", yolo_flag: null },
        codex: {
          command: "codex",
          yolo_flag: "--dangerously-bypass-approvals-and-sandbox",
          resume_command: "codex resume --last",
          prompt_command: "{prompt}",
        },
      },
    });

    unmount(component);
  });
});
