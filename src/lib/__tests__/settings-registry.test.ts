import { describe, expect, it } from "vitest";
import type { AppConfig } from "../settings.svelte";
import {
  SETTINGS,
  SETTINGS_CATEGORIES,
  isModified,
  parseSettingsLocation,
  resetPatch,
  searchSettings,
  settingById,
  settingsLocationQuery,
} from "../settings-registry";

function config(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: { kiro: { command: "kiro-cli chat", yolo_flag: null } },
    default_provider: "kiro",
    auto_open_review: false,
    sound_enabled: true,
    ...overrides,
  };
}

const defaults = config();

describe("settings registry", () => {
  it("gives every setting a unique id in a known category", () => {
    const ids = SETTINGS.map((setting) => setting.id);
    expect(new Set(ids).size).toBe(ids.length);
    const categories = new Set(SETTINGS_CATEGORIES.map((category) => category.id));
    for (const setting of SETTINGS) expect(categories.has(setting.category), setting.id).toBe(true);
  });

  it("has no catch-all category", () => {
    expect(SETTINGS_CATEGORIES.map((category) => category.label)).not.toContain("More");
  });

  it("puts every setting that used to live under More in a workflow category", () => {
    expect(settingById("session-backend")?.category).toBe("sessions");
    expect(settingById("vim-mode")?.category).toBe("editor");
    expect(settingById("post-merge-action")?.category).toBe("general");
    expect(settingById("projects-folder")?.category).toBe("general");
    expect(settingById("cli")?.category).toBe("advanced");
    expect(settingById("logs")?.category).toBe("advanced");
  });
});

describe("searchSettings", () => {
  it("matches labels, keywords and section names case-insensitively", () => {
    expect(searchSettings("TMUX", [], defaults).map((r) => r.id)).toContain("session-backend");
    expect(searchSettings("font", [], defaults).map((r) => r.id)).toEqual(
      expect.arrayContaining(["font-family", "font-size"]),
    );
    expect(searchSettings("scrollback", [], defaults).map((r) => r.id)).toContain(
      "scrollback-lines",
    );
  });

  it("requires every word of a multi-word query", () => {
    const ids = searchSettings("font size", [], defaults).map((r) => r.id);
    expect(ids).toContain("font-size");
    expect(ids).not.toContain("font-family");
  });

  it("ranks label matches before keyword-only matches", () => {
    const ids = searchSettings("theme", [], defaults).map((r) => r.id);
    expect(ids[0]).toBe("theme");
  });

  it("leaves out settings their page does not show", () => {
    expect(searchSettings("branch", [], config()).map((r) => r.id)).not.toContain(
      "branch-template",
    );
    const withTasks = config({ task_management: {} });
    expect(searchSettings("branch", [], withTasks).map((r) => r.id)).toContain("branch-template");
  });

  it("returns nothing for a blank query", () => {
    expect(searchSettings("   ", [], defaults)).toEqual([]);
  });

  it("finds plugin pages by plugin name and contribution label", () => {
    const pages = [
      {
        key: "jira:preferences",
        label: "Jira",
        pluginName: "Jira Cloud",
        description: "Sync issues",
      },
    ];
    expect(searchSettings("cloud", pages, defaults)).toEqual([
      expect.objectContaining({ kind: "plugin", key: "jira:preferences", label: "Jira" }),
    ]);
    expect(searchSettings("sync", pages, defaults).map((r) => r.id)).toContain("jira:preferences");
  });
});

describe("modified and reset", () => {
  it("treats an unset optional field as its effective default", () => {
    const vim = settingById("vim-mode")!;
    expect(isModified(vim, config(), defaults)).toBe(false);
    expect(isModified(vim, config({ vim_mode: true }), defaults)).toBe(false);
    expect(isModified(vim, config({ vim_mode: false }), defaults)).toBe(true);
  });

  it("resets an optional field by clearing it", () => {
    const vim = settingById("vim-mode")!;
    expect(resetPatch(vim, config({ vim_mode: false }), defaults)).toEqual({ vim_mode: null });
  });

  it("resets a nested field without touching its siblings", () => {
    const size = settingById("font-size")!;
    const current = config({
      terminal: { font_family: "Fira Code", font_size: 18, option_as_meta: false },
    });
    expect(isModified(size, current, defaults)).toBe(true);
    expect(resetPatch(size, current, defaults)).toEqual({
      terminal: { font_family: "Fira Code", font_size: 14, option_as_meta: false },
    });
  });

  it("never reports a change before backend defaults have loaded", () => {
    expect(isModified(settingById("vim-mode")!, config({ vim_mode: false }), null)).toBe(false);
  });

  it("does not offer reset for list-shaped settings", () => {
    expect(settingById("agents")?.value).toBeUndefined();
    expect(settingById("language-server-profiles")?.value).toBeUndefined();
  });
});

describe("settings location", () => {
  it("defaults to General", () => {
    expect(parseSettingsLocation("?page=preferences", "")).toEqual({
      kind: "category",
      id: "general",
    });
  });

  it("reads a section and a setting anchor", () => {
    expect(parseSettingsLocation("?page=preferences&section=agents", "#default-agent")).toEqual({
      kind: "category",
      id: "agents",
      settingId: "default-agent",
    });
  });

  it("derives the category from a setting anchor alone", () => {
    expect(parseSettingsLocation("?page=preferences", "#vim-mode")).toEqual({
      kind: "category",
      id: "editor",
      settingId: "vim-mode",
    });
  });

  it("opens plugin pages", () => {
    expect(parseSettingsLocation("?page=preferences&plugin=jira%3Apreferences", "")).toEqual({
      kind: "plugin",
      key: "jira:preferences",
    });
  });

  it("ignores unknown sections and anchors", () => {
    expect(parseSettingsLocation("?page=preferences&section=nope", "#nope")).toEqual({
      kind: "category",
      id: "general",
    });
  });

  it("round-trips through the window query", () => {
    const location = { kind: "category", id: "agents", settingId: "default-agent" } as const;
    const [search, hash] = settingsLocationQuery(location);
    expect(parseSettingsLocation(search, hash)).toEqual(location);
  });
});
