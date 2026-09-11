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

afterEach(() => {
  mocks.updateSettings.mockClear();
  document.body.replaceChildren();
});

import PreferencesPage from "../PreferencesPage.svelte";

function setInput(selector: string, value: string) {
  const input = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
  if (!input) throw new Error(`Missing ${selector}`);
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("PreferencesPage language servers", () => {
  it("creates a validated custom profile through the typed settings update", async () => {
    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(PreferencesPage, { target });
    await tick();

    Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.trim() === "More")?.click();
    await tick();
    Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Add profile")?.click();
    await tick();

    setInput("#lsp-profile-id", "local-rust-analyzer");
    setInput("#lsp-profile-language", "rust");
    setInput("#lsp-profile-extensions", ".rs, rsi");
    setInput("#lsp-profile-command", "/opt/tools/rust-analyzer");
    setInput("#lsp-profile-args", "--stdio\n--config\ncheck.command=clippy");

    Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Save profile")?.click();
    await tick();

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      language_servers: {
        enabled: true,
        profiles: [{
          id: "local-rust-analyzer",
          language_id: "rust",
          extensions: ["rs", "rsi"],
          command: "/opt/tools/rust-analyzer",
          args: ["--stdio", "--config", "check.command=clippy"],
          enabled: true,
        }],
      },
    });

    unmount(component);
  });
});
