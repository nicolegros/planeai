import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";
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
import { isKeyboardSuspended } from "../../lib/keyboard";

let component: ReturnType<typeof mount> | undefined;

async function flush() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
  }
}

async function render() {
  const target = document.body.appendChild(document.createElement("div"));
  component = mount(Onboarding, { target });
  await vi.waitFor(() => expect(target.textContent).not.toContain("Looking for agents"));
  await flush();
  return target;
}

function button(label: string): HTMLButtonElement {
  const match = Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!match) throw new Error(`Missing ${label} button`);
  return match;
}

async function click(label: string) {
  button(label).click();
  await flush();
}

function agent(label: string): HTMLButtonElement {
  const match = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="checkbox"]')).find(
    (row) => row.textContent?.trim().startsWith(label),
  );
  if (!match) throw new Error(`Missing ${label} agent`);
  return match;
}

beforeEach(() => {
  mocks.config = firstLaunchConfig();
  mocks.updateSettings.mockResolvedValue(undefined);
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
    await click("Continue");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      providers: { claude: preset("claude"), codex: preset("codex") },
      default_provider: "claude",
    });
    expect(document.body.textContent).toContain("Where do your projects live?");
  });

  it("finishes immediately when skipped", async () => {
    await render();
    await click("Skip setup");
    expect(mocks.updateSettings).toHaveBeenCalledExactlyOnceWith({ onboarding_completed: true });
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
    await click("Add a folder to PATH…");
    expect(mocks.updateSettings).toHaveBeenCalledWith({ extra_path_dirs: ["/opt/tools"] });
    await vi.waitFor(() => expect(agent("Claude Code").getAttribute("aria-checked")).toBe("true"));
  });

  it("keeps the configured agents when none is selected", async () => {
    mocks.detectProviders.mockResolvedValue({
      kiro: null,
      claude: null,
      copilot: null,
      codex: null,
    });
    await render();
    await click("Continue");
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Where do your projects live?");
  });

  it("saves the projects folder only when it changed", async () => {
    await render();
    await click("Continue");
    mocks.updateSettings.mockClear();
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Projects folder"]')!;
    input.value = "~/Developer";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click("Continue");
    expect(mocks.updateSettings).toHaveBeenCalledWith({ projects_base_path: "~/Developer" });
    mocks.updateSettings.mockClear();
    await click("Back");
    await click("Continue");
    expect(mocks.updateSettings).toHaveBeenCalledWith({ projects_base_path: "~/Developer" });
  });

  it("applies the look right away and finishes setup", async () => {
    await render();
    await click("Continue");
    await click("Continue");
    document
      .querySelector<HTMLInputElement>('input[name="onboarding-appearance-mode"][value="dark"]')!
      .click();
    await flush();
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      appearance: { mode: "dark", theme: "default" },
    });
    await click("Finish");
    expect(mocks.updateSettings).toHaveBeenLastCalledWith({ onboarding_completed: true });
  });

  it("stays on the step when saving it fails", async () => {
    await render();
    mocks.updateSettings.mockRejectedValueOnce("disk full");
    await click("Continue");
    expect(document.body.textContent).toContain("Which agents do you use?");
  });

  it("suspends app shortcuts while it is open", async () => {
    await render();
    expect(isKeyboardSuspended()).toBe(true);
    unmount(component!);
    component = undefined;
    expect(isKeyboardSuspended()).toBe(false);
  });
});
