import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import type { AppConfig } from "../../lib/settings.svelte";
import { PROVIDER_PRESETS } from "../../lib/provider-presets";

const preset = (key: string) =>
  PROVIDER_PRESETS.find((candidate) => candidate.key === key)!.provider;

function firstLaunchConfig(): AppConfig {
  return {
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {
      kiro: preset("kiro"),
      claude: preset("claude"),
      copilot: preset("copilot"),
      codex: preset("codex"),
    },
    default_provider: "kiro",
    onboarding_completed: false,
  };
}

const mocks = vi.hoisted(() => ({
  config: {} as AppConfig,
  updateSettings: vi.fn(),
  detectProviders: vi.fn(),
  openDialog: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    config: { ...actual.config, detectProviders: mocks.detectProviders },
    preferences: {
      ...actual.preferences,
      listThemes: vi.fn(() => Promise.resolve(["default", "nord"])),
      listTerminalSchemes: vi.fn(() => Promise.resolve([])),
    },
  };
});
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => mocks.config,
  updateSettings: mocks.updateSettings,
}));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.openDialog }));

import Onboarding from "../Onboarding.svelte";

let component: ReturnType<typeof mount> | undefined;

async function render() {
  const target = document.body.appendChild(document.createElement("div"));
  component = mount(Onboarding, { target });
  await vi.waitFor(() => expect(target.textContent).not.toContain("Looking for agents"));
  return target;
}

function button(label: string): HTMLButtonElement {
  const match = Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!match) throw new Error(`Missing ${label} button`);
  return match;
}

function agent(label: string): HTMLButtonElement {
  const match = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="checkbox"]')).find(
    (row) => row.textContent?.trim().startsWith(label),
  );
  if (!match) throw new Error(`Missing ${label} agent`);
  return match;
}

const title = () => document.querySelector("h1")?.textContent;

async function continueTo(nextTitle: string) {
  button("Continue").click();
  await vi.waitFor(() => expect(title()).toBe(nextTitle));
}

beforeEach(() => {
  mocks.config = firstLaunchConfig();
  // Behaves like the store: a successful save is visible to later reads of the same config.
  mocks.updateSettings.mockImplementation(async (patch: Partial<AppConfig>) => {
    Object.assign(mocks.config, patch);
  });
  mocks.detectProviders.mockResolvedValue({
    kiro: null,
    claude: "/opt/homebrew/bin/claude",
    copilot: null,
    codex: "/usr/local/bin/codex",
  });
});

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe("onboarding", () => {
  it("pre-selects the agents found on PATH and saves them on Continue", async () => {
    await render();
    expect(agent("Claude Code").getAttribute("aria-checked")).toBe("true");
    expect(agent("Codex").getAttribute("aria-checked")).toBe("true");
    expect(agent("Kiro").getAttribute("aria-checked")).toBe("false");
    await continueTo("Where do your projects live?");
    expect(mocks.updateSettings).toHaveBeenCalledExactlyOnceWith({
      providers: { claude: preset("claude"), codex: preset("codex") },
      default_provider: "claude",
    });
  });

  it("keeps a custom default agent", async () => {
    mocks.config = {
      ...firstLaunchConfig(),
      providers: { ...firstLaunchConfig().providers, aider: { command: "aider", yolo_flag: null } },
      default_provider: "aider",
    };
    await render();
    await continueTo("Where do your projects live?");
    expect(mocks.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ default_provider: "aider" }),
    );
  });

  it("finishes immediately when skipped", async () => {
    await render();
    button("Skip setup").click();
    await vi.waitFor(() =>
      expect(mocks.updateSettings).toHaveBeenCalledExactlyOnceWith({ onboarding_completed: true }),
    );
  });

  it("offers to add a PATH folder when no agent is found, then selects what it finds", async () => {
    mocks.detectProviders.mockResolvedValueOnce({
      kiro: null,
      claude: null,
      copilot: null,
      codex: null,
    });
    mocks.detectProviders.mockResolvedValueOnce({
      kiro: null,
      claude: "/opt/tools/claude",
      copilot: null,
      codex: null,
    });
    mocks.openDialog.mockResolvedValue("/opt/tools");
    await render();
    expect(document.body.textContent).toContain("No agents found on your PATH.");
    button("Add a folder to PATH…").click();
    await vi.waitFor(() => expect(agent("Claude Code").getAttribute("aria-checked")).toBe("true"));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ extra_path_dirs: ["/opt/tools"] });
  });

  it("keeps the configured agents when none is selected", async () => {
    mocks.detectProviders.mockResolvedValue({
      kiro: null,
      claude: null,
      copilot: null,
      codex: null,
    });
    await render();
    await continueTo("Where do your projects live?");
    expect(mocks.updateSettings).not.toHaveBeenCalled();
  });

  it("saves the projects folder only when it changed", async () => {
    await render();
    await continueTo("Where do your projects live?");
    await continueTo("Pick a look");
    expect(mocks.updateSettings).toHaveBeenCalledTimes(1);

    button("Back").click();
    await vi.waitFor(() => expect(title()).toBe("Where do your projects live?"));
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Projects folder"]')!;
    input.value = "~/Developer";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await continueTo("Pick a look");
    expect(mocks.updateSettings).toHaveBeenLastCalledWith({ projects_base_path: "~/Developer" });

    button("Back").click();
    await vi.waitFor(() => expect(title()).toBe("Where do your projects live?"));
    await continueTo("Pick a look");
    expect(mocks.updateSettings).toHaveBeenCalledTimes(2);
  });

  it("applies the look right away and finishes setup", async () => {
    await render();
    await continueTo("Where do your projects live?");
    await continueTo("Pick a look");
    document
      .querySelector<HTMLInputElement>('input[name="onboarding-appearance-mode"][value="dark"]')!
      .click();
    await vi.waitFor(() =>
      expect(mocks.updateSettings).toHaveBeenCalledWith({
        appearance: { mode: "dark", theme: "default" },
      }),
    );
    button("Finish").click();
    await vi.waitFor(() =>
      expect(mocks.updateSettings).toHaveBeenLastCalledWith({ onboarding_completed: true }),
    );
  });

  it("stays on the step when saving it fails", async () => {
    await render();
    mocks.updateSettings.mockRejectedValueOnce("disk full");
    button("Continue").click();
    await vi.waitFor(() => expect(button("Continue").disabled).toBe(false));
    expect(title()).toBe("Which agents do you use?");
  });

  it("takes focus on open and on every step", async () => {
    const terminal = document.body.appendChild(document.createElement("textarea"));
    terminal.focus();
    await render();
    await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector("h1")));
    await continueTo("Where do your projects live?");
    await vi.waitFor(() =>
      expect(document.activeElement?.textContent).toBe("Where do your projects live?"),
    );
  });

  it("keeps keys typed outside the wizard from reaching workspace shortcuts", async () => {
    const workspaceShortcut = vi.fn();
    window.addEventListener("keydown", workspaceShortcut);
    try {
      await render();
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
      expect(workspaceShortcut).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(document.querySelector("h1"));

      document
        .querySelector("h1")!
        .dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
      expect(workspaceShortcut).toHaveBeenCalledOnce();
    } finally {
      window.removeEventListener("keydown", workspaceShortcut);
    }
  });
});
