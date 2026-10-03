import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";
import type { AppConfig } from "../../lib/settings.svelte";

function baseConfig(): AppConfig {
  return {
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } },
    default_provider: "kiro",
    auto_open_review: false,
    sound_enabled: true,
  };
}

const mocks = vi.hoisted(() => ({
  config: {} as AppConfig,
  updateSettings: vi.fn(),
  refreshSettings: vi.fn(),
  loadSettings: vi.fn(),
  showSnackbar: vi.fn(),
  defaults: vi.fn(),
  detectProviders: vi.fn(),
  checkRmuxAvailable: vi.fn(),
  getVersion: vi.fn(),
  getPending: vi.fn(),
  check: vi.fn(),
  install: vi.fn(),
  listen: vi.fn(),
  pluginsList: vi.fn(),
  closeWindow: vi.fn(),
  focusMain: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...actual,
    config: { ...actual.config, defaults: mocks.defaults, detectProviders: mocks.detectProviders },
    updater: {
      getVersion: mocks.getVersion,
      getPending: mocks.getPending,
      check: mocks.check,
      install: mocks.install,
    },
    preferences: {
      ...actual.preferences,
      listMonospaceFonts: vi.fn(() => Promise.resolve([])),
      listThemes: vi.fn(() => Promise.resolve(["default", "nord"])),
      checkTmuxAvailable: vi.fn(() => Promise.resolve(true)),
      checkRmuxAvailable: mocks.checkRmuxAvailable,
      checkCliInstalled: vi.fn(() => Promise.resolve(true)),
      listStaleWorktrees: vi.fn(() => Promise.resolve([])),
      runStaleWorktreeCleanup: vi.fn(() => Promise.resolve([])),
      installCli: vi.fn(),
      getLogDir: vi.fn(() => Promise.resolve("/tmp")),
    },
    plugins: {
      ...actual.plugins,
      list: mocks.pluginsList,
      jiraMigrationStatus: vi.fn(() => Promise.resolve(null)),
      githubMigrationStatus: vi.fn(() => Promise.resolve(null)),
    },
  };
});
vi.mock("../../lib/settings.svelte", () => ({
  loadSettings: mocks.loadSettings,
  getSettings: () => mocks.config,
  updateSettings: mocks.updateSettings,
  refreshSettings: mocks.refreshSettings,
}));
vi.mock("../../lib/theme-loader", () => ({ loadTheme: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: mocks.showSnackbar }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: mocks.closeWindow }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: {
    getByLabel: vi.fn(async (label: string) =>
      label === "main" ? { setFocus: mocks.focusMain } : null,
    ),
  },
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn(), openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen, emit: vi.fn() }));

import SettingsWindow from "../settings/SettingsWindow.svelte";
import { settingsWindow } from "../settings/settings-window.svelte";
import { _resetForTests } from "../../lib/updater.svelte";

async function flush() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
  }
}

let component: ReturnType<typeof mount> | undefined;

async function render(url = "/?page=preferences") {
  window.history.replaceState(null, "", url);
  const target = document.body.appendChild(document.createElement("div"));
  component = mount(SettingsWindow, { target });
  await vi.waitFor(() => expect(target.textContent).not.toContain("Loading settings"));
  await flush();
  return target;
}

function button(label: string): HTMLButtonElement | undefined {
  return Array.from(document.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
}

async function click(label: string) {
  const target = button(label);
  if (!target) throw new Error(`Missing ${label} button`);
  target.click();
  await flush();
}

function toggle(label: string): HTMLButtonElement {
  const target = document.querySelector<HTMLButtonElement>(
    `[role="switch"][aria-label="${label}"]`,
  );
  if (!target) throw new Error(`Missing ${label} switch`);
  return target;
}

function radio(name: string, value: string): HTMLInputElement {
  const target = document.querySelector<HTMLInputElement>(
    `input[name="${name}"][value="${value}"]`,
  );
  if (!target) throw new Error(`Missing ${name}=${value} radio`);
  return target;
}

function setInput(selector: string, value: string) {
  const input = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
  if (!input) throw new Error(`Missing ${selector}`);
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

async function search(query: string) {
  setInput('input[aria-label="Search settings"]', query);
  await flush();
}

// jsdom does not implement scrolling.
const scrollIntoView = vi.fn();
Element.prototype.scrollIntoView = scrollIntoView;

beforeEach(() => {
  mocks.config = baseConfig();
  mocks.defaults.mockResolvedValue(baseConfig());
  mocks.detectProviders.mockResolvedValue({});
  mocks.checkRmuxAvailable.mockResolvedValue(true);
  mocks.getVersion.mockResolvedValue("1.80.0");
  mocks.getPending.mockResolvedValue(null);
  mocks.listen.mockResolvedValue(() => {});
  mocks.pluginsList.mockResolvedValue([]);
  mocks.updateSettings.mockResolvedValue(undefined);
  settingsWindow.query = "";
  settingsWindow.defaults = null;
  settingsWindow.editorDraft = null;
  _resetForTests();
});

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe("navigation", () => {
  it("lists workflow categories with Advanced last and no catch-all", async () => {
    await render();
    const labels = Array.from(
      document.querySelectorAll('[aria-label="Settings categories"] nav button'),
    ).map((el) => el.textContent?.trim());
    expect(labels).toEqual([
      "General",
      "Appearance",
      "Terminal",
      "Agents",
      "Sessions",
      "Tasks",
      "Editor",
      "Plugins",
      "Advanced",
    ]);
  });

  it("opens the category and setting from a deep link", async () => {
    await render("/?page=preferences&section=editor#vim-mode");
    expect(document.querySelector("h1")?.textContent).toBe("Editor");
    expect(settingsWindow.flashId).toBe("vim-mode");
    await vi.waitFor(() =>
      expect(scrollIntoView.mock.contexts).toContain(document.getElementById("setting-vim-mode")),
    );
  });

  it("listens for navigation before loading, so an early deep link is not dropped", async () => {
    await render();
    const listenOrder =
      mocks.listen.mock.invocationCallOrder[
        mocks.listen.mock.calls.findIndex(([name]) => name === "preferences-navigate")
      ];
    expect(listenOrder).toBeLessThan(mocks.loadSettings.mock.invocationCallOrder[0]);
  });

  it("shows loading, not an error, while a plugin deep link waits for the plugin list", async () => {
    let resolvePlugins!: (list: unknown[]) => void;
    mocks.pluginsList.mockReturnValue(new Promise((resolve) => (resolvePlugins = resolve)));
    await render("/?page=preferences&plugin=jira%3Apreferences");
    expect(document.body.textContent).toContain("Loading plugin…");
    expect(document.body.textContent).not.toContain("not available");
    resolvePlugins([]);
    await flush();
    expect(document.body.textContent).toContain("This plugin page is not available");
  });

  it("nests plugin preference pages under Plugins and opens them", async () => {
    mocks.pluginsList.mockResolvedValue([
      {
        id: "jira",
        name: "Jira",
        state: "running",
        ui_contributions: [
          { id: "preferences", label: "Jira", placement: "preferences", entrypoint: "ui.js" },
        ],
      },
    ]);
    await render();
    const labels = Array.from(
      document.querySelectorAll('[aria-label="Settings categories"] nav button'),
    ).map((el) => el.textContent?.trim());
    expect(labels.slice(labels.indexOf("Plugins"), labels.indexOf("Plugins") + 2)).toEqual([
      "Plugins",
      "Jira",
    ]);
    await click("Jira");
    expect(document.querySelector("h1")?.textContent).toBe("Jira");
    expect(
      document.querySelector('[data-plugin-preference-contribution="jira:preferences"]'),
    ).not.toBeNull();
  });
});

describe("search", () => {
  it("shows matching settings with breadcrumbs and jumps to one", async () => {
    await render();
    await search("tmux");
    expect(document.querySelector("h1")?.textContent).toContain("tmux");
    const result = document.querySelector('[aria-label="Search results"] button');
    expect(result?.textContent).toContain("Sessions › Backend");
    (result as HTMLButtonElement).click();
    await flush();
    expect(document.querySelector("h1")?.textContent).toBe("Sessions");
    expect(settingsWindow.query).toBe("");
    expect(settingsWindow.flashId).toBe("session-backend");
  });

  it("opens the first result on Enter", async () => {
    await render();
    await search("vim");
    document
      .querySelector('input[aria-label="Search settings"]')!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await flush();
    expect(document.querySelector("h1")?.textContent).toBe("Editor");
  });

  it("says when nothing matches", async () => {
    await render();
    await search("zzzz");
    expect(document.body.textContent).toContain("No settings match");
  });
});

describe("reset to default", () => {
  it("offers reset only for changed settings and restores the backend default", async () => {
    mocks.config.vim_mode = false;
    await render("/?page=preferences&section=editor");
    expect(
      document.querySelector('[aria-label="Reset Vim keybindings to default"]'),
    ).not.toBeNull();
    expect(document.querySelector('[aria-label="Reset Code intelligence to default"]')).toBeNull();

    (
      document.querySelector('[aria-label="Reset Vim keybindings to default"]') as HTMLButtonElement
    ).click();
    await flush();
    expect(mocks.updateSettings).toHaveBeenCalledWith({ vim_mode: null });
  });

  it("hides reset until defaults load", async () => {
    mocks.defaults.mockReturnValue(new Promise(() => {}));
    mocks.config.vim_mode = false;
    await render("/?page=preferences&section=editor");
    expect(document.querySelector('[aria-label="Reset Vim keybindings to default"]')).toBeNull();
  });
});

describe("appearance", () => {
  it("defaults to project grouping with project labels disabled", async () => {
    await render("/?page=preferences&section=appearance");
    expect(radio("sidebar-group-by", "project").checked).toBe(true);
    expect(radio("sidebar-group-by", "status").checked).toBe(false);
    expect(toggle("Show task keys").getAttribute("aria-checked")).toBe("true");
    expect(toggle("Show project labels").disabled).toBe(true);
  });

  it("saves the grouping and label choices", async () => {
    mocks.config.sidebar_group_by = "status";
    await render("/?page=preferences&section=appearance");
    radio("sidebar-group-by", "project").click();
    toggle("Show task keys").click();
    expect(toggle("Show project labels").disabled).toBe(false);
    toggle("Show project labels").click();
    toggle("Hide empty projects").click();
    await flush();
    expect(mocks.updateSettings.mock.calls.map(([patch]) => patch)).toEqual([
      { sidebar_group_by: "project" },
      { hide_task_keys: true },
      { hide_project_labels: true },
      { hide_empty_projects: true },
    ]);
  });
});

describe("terminal", () => {
  it("saves scrollback and clickable links, which used to be config-only", async () => {
    await render("/?page=preferences&section=terminal");
    const scrollback = document.querySelector<HTMLInputElement>(
      'input[aria-label="Scrollback lines"]',
    )!;
    expect(scrollback.value).toBe("20000");
    scrollback.value = "50000";
    scrollback.dispatchEvent(new Event("change", { bubbles: true }));
    toggle("Clickable links").click();
    await flush();
    expect(mocks.updateSettings.mock.calls.map(([patch]) => patch)).toEqual([
      { scrollback_lines: 50000 },
      { web_links: false },
    ]);
  });
});

describe("agents", () => {
  it("lists every preset, with detection status", async () => {
    mocks.detectProviders.mockResolvedValue({ kiro: "/usr/local/bin/kiro-cli", claude: null });
    await render("/?page=preferences&section=agents");
    const rows = Array.from(document.querySelectorAll("[data-agent]")).map((row) =>
      row.getAttribute("data-agent"),
    );
    expect(rows).toEqual(["kiro", "claude", "copilot", "codex"]);
    await vi.waitFor(() =>
      expect(document.querySelector('[data-agent="kiro"]')?.textContent).toContain(
        "/usr/local/bin/kiro-cli",
      ),
    );
    expect(document.querySelector('[data-agent="claude"]')?.textContent).toContain(
      "Not found on PATH",
    );
  });

  it("turns a preset on with its full entry and re-detects afterwards", async () => {
    await render("/?page=preferences&section=agents");
    mocks.detectProviders.mockClear();
    toggle("Enable Codex").click();
    await flush();
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
    expect(mocks.detectProviders).toHaveBeenCalledOnce();
  });

  it("names the default agent picker for assistive technology", async () => {
    await render("/?page=preferences&section=agents");
    expect(document.querySelector('input[aria-label="Default agent"]')).not.toBeNull();
  });

  it("keeps only the latest detection when detections finish out of order", async () => {
    await render("/?page=preferences&section=agents");
    const pending: ((value: Record<string, string | null>) => void)[] = [];
    mocks.detectProviders.mockImplementation(() => new Promise((resolve) => pending.push(resolve)));
    toggle("Enable Codex").click();
    toggle("Enable Copilot").click();
    await vi.waitFor(() => expect(pending).toHaveLength(2));
    pending[1]({ codex: "/bin/codex", copilot: "/bin/copilot" });
    await flush();
    pending[0]({ codex: "/bin/codex" });
    await flush();
    expect(document.querySelector('[data-agent="copilot"]')?.textContent).toContain("/bin/copilot");
  });

  it("reports a failed agent update and still refreshes detection", async () => {
    await render("/?page=preferences&section=agents");
    mocks.detectProviders.mockClear();
    mocks.updateSettings.mockRejectedValueOnce("invalid config");
    toggle("Enable Codex").click();
    await flush();
    expect(mocks.showSnackbar).toHaveBeenCalledWith(
      "Failed to save settings: invalid config",
      "error",
    );
    expect(mocks.detectProviders).toHaveBeenCalledOnce();
  });

  it("highlights the agents list when a search jumps to it", async () => {
    await render();
    await search("agents");
    const result = Array.from(
      document.querySelectorAll('[aria-label="Search results"] button'),
    ).find((el) => el.textContent?.includes("Agents › Agents"));
    (result as HTMLButtonElement).click();
    await flush();
    expect(document.getElementById("setting-agents")?.className).toContain("ring-status-review");
  });

  it("locks the default agent's switch", async () => {
    await render("/?page=preferences&section=agents");
    expect(toggle("Enable Kiro").disabled).toBe(true);
    expect(toggle("Enable Kiro").title).toBe("Choose another default first");
  });

  it("adds a custom agent through the dialog", async () => {
    await render("/?page=preferences&section=agents");
    await click("Add custom agent");
    setInput("#agent-name", "goose");
    setInput("#agent-command", "goose session");
    await click("Add agent");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      providers: {
        kiro: { command: "kiro-cli chat", yolo_flag: null },
        goose: { command: "goose session", yolo_flag: null },
      },
    });
  });

  it("rejects a custom agent that reuses a preset name", async () => {
    await render("/?page=preferences&section=agents");
    await click("Add custom agent");
    setInput("#agent-name", "codex");
    setInput("#agent-command", "codex");
    await click("Add agent");
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("codex is a built-in agent");
  });
});

describe("sessions", () => {
  it("offers every backend and persists rmux", async () => {
    await render("/?page=preferences&section=sessions");
    for (const value of ["local", "tmux", "daemon", "rmux"]) radio("session-backend", value);
    radio("session-backend", "rmux").click();
    await flush();
    expect(mocks.updateSettings).toHaveBeenCalledWith({ session_backend: "rmux" });
  });

  it("stores local as the unset default", async () => {
    mocks.config.session_backend = "tmux";
    await render("/?page=preferences&section=sessions");
    radio("session-backend", "local").click();
    await flush();
    expect(mocks.updateSettings).toHaveBeenCalledWith({ session_backend: null });
  });

  it("warns when rmux is selected but missing, and not otherwise", async () => {
    mocks.config.session_backend = "rmux";
    mocks.checkRmuxAvailable.mockResolvedValue(false);
    await render("/?page=preferences&section=sessions");
    expect(document.body.textContent).toContain("rmux not found on PATH");
  });

  it("describes rmux without a warning when present", async () => {
    mocks.config.session_backend = "rmux";
    await render("/?page=preferences&section=sessions");
    expect(document.body.textContent).not.toContain("rmux not found on PATH");
    expect(document.body.textContent).toContain("requires rmux");
  });
});

describe("tasks", () => {
  it("turns tasks on with the backend's recommended setup, fetching it if needed", async () => {
    mocks.defaults.mockRejectedValueOnce("offline");
    await render("/?page=preferences&section=tasks");
    const recommended = { on_start: { move_to: "in_progress" } };
    mocks.defaults.mockResolvedValueOnce({ ...baseConfig(), task_management: recommended });
    toggle("Task management").click();
    await flush();
    expect(mocks.updateSettings).toHaveBeenCalledWith({ task_management: recommended });
  });

  it("does not offer task settings in search while tasks are off", async () => {
    await render();
    await search("branch");
    expect(document.body.textContent).toContain("No settings match");
  });

  it("shows an unset hook as disabled instead of its suggested status", async () => {
    mocks.config.task_management = { on_start: { move_to: "in_progress" } };
    await render("/?page=preferences&section=tasks");
    const hook = (key: string) => document.querySelector<HTMLInputElement>(`#hook-${key}`)!;
    expect(hook("on_start").value).toBe("in_progress");
    expect(hook("on_notify").value).toBe("");
    expect(hook("on_notify").placeholder).toBe("Disabled - e.g. in_review");
    expect(hook("on_resume").placeholder).toBe("Disabled - e.g. in_progress");
  });
});

describe("editor", () => {
  it("saves an external editor preset as typed editor settings", async () => {
    await render("/?page=preferences&section=editor");
    radio("file-editor-mode", "external").click();
    await flush();
    await click("VS Code");
    await click("Save editor settings");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      editor: { mode: "external", command: "code", args: ["--reuse-window", "--goto", "{file}"] },
    });
  });

  it("keeps unsaved editor edits across page switches and searches", async () => {
    await render("/?page=preferences&section=editor");
    radio("file-editor-mode", "external").click();
    await flush();
    await click("VS Code");
    await click("General");
    await search("vim");
    document.querySelector<HTMLButtonElement>('[aria-label="Clear search"]')!.click();
    await flush();
    await click("Editor");
    expect(radio("file-editor-mode", "external").checked).toBe(true);
    await click("Save editor settings");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      editor: { mode: "external", command: "code", args: ["--reuse-window", "--goto", "{file}"] },
    });
  });

  it("does not save an incomplete terminal editor configuration", async () => {
    await render("/?page=preferences&section=editor");
    radio("file-editor-mode", "terminal").click();
    await flush();
    await click("Save editor settings");
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Editor executable is required.");
  });

  it("rebuilds the editor draft from a config reloaded on the Advanced page", async () => {
    mocks.refreshSettings.mockImplementation(async () => {
      mocks.config.editor = { mode: "terminal", command: "nvim", args: ["{file}"] };
    });
    await render("/?page=preferences&section=editor");
    radio("file-editor-mode", "external").click();
    await flush();
    await click("VS Code");
    await click("Advanced");
    await click("Reload");
    await click("Editor");
    await click("Save editor settings");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      editor: { mode: "terminal", command: "nvim", args: ["{file}"] },
    });
  });

  it("creates a validated language-server profile", async () => {
    await render("/?page=preferences&section=editor");
    await click("Add profile");
    setInput("#lsp-profile-id", "local-rust-analyzer");
    setInput("#lsp-profile-language", "rust");
    setInput("#lsp-profile-extensions", ".rs, rsi");
    setInput("#lsp-profile-command", "/opt/tools/rust-analyzer");
    setInput("#lsp-profile-args", "--stdio\n--config\ncheck.command=clippy");
    await click("Save profile");
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      language_servers: {
        enabled: true,
        profiles: [
          {
            id: "local-rust-analyzer",
            language_id: "rust",
            extensions: ["rs", "rsi"],
            command: "/opt/tools/rust-analyzer",
            args: ["--stdio", "--config", "check.command=clippy"],
            enabled: true,
          },
        ],
      },
    });
  });

  it("renders a saved profile whose empty args were omitted", async () => {
    mocks.config.language_servers = {
      profiles: [
        {
          id: "local-rust-analyzer",
          language_id: "rust",
          extensions: ["rs"],
          command: "rust-analyzer",
        },
      ],
    };
    await render("/?page=preferences&section=editor");
    expect(document.body.textContent).toContain("local-rust-analyzer");
    expect(document.body.textContent).toContain(".rs");
  });
});

describe("setup assistant", () => {
  it("restarts onboarding in the main window and closes Preferences", async () => {
    await render();
    await click("Run setup again");
    expect(mocks.updateSettings).toHaveBeenCalledWith({ onboarding_completed: false });
    expect(mocks.focusMain).toHaveBeenCalledOnce();
    expect(mocks.closeWindow).toHaveBeenCalledOnce();
  });

  it("still closes Preferences when the main window cannot be focused", async () => {
    mocks.focusMain.mockRejectedValueOnce("not allowed");
    await render();
    await click("Run setup again");
    expect(mocks.closeWindow).toHaveBeenCalledOnce();
  });

  it("stays open when the flag could not be saved", async () => {
    mocks.updateSettings.mockRejectedValueOnce("disk full");
    await render();
    await click("Run setup again");
    expect(mocks.closeWindow).not.toHaveBeenCalled();
  });
});

describe("app updates", () => {
  it("checks for and installs a manually discovered release from General", async () => {
    mocks.check.mockResolvedValue({ version: "1.81.0", body: null });
    mocks.install.mockResolvedValue(undefined);
    await render();
    await vi.waitFor(() => expect(document.body.textContent).toContain("PlaneAI v1.80.0"));
    await click("Check for updates");
    expect(mocks.check).toHaveBeenCalledOnce();
    expect(document.body.textContent).toContain("Update available: v1.81.0");
    await click("Install & Restart");
    expect(mocks.install).toHaveBeenCalledOnce();
    expect(document.body.textContent).toContain("Downloading and installing v1.81.0");
  });

  it("shows a launch-discovered update retained by the backend", async () => {
    mocks.getPending.mockResolvedValue({ version: "1.81.0", body: null });
    await render();
    await vi.waitFor(() =>
      expect(document.body.textContent).toContain("Update available: v1.81.0"),
    );
    expect(button("Install & Restart")).toBeTruthy();
  });
});
