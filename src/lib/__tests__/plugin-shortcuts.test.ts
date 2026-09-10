import { describe, expect, it } from "vitest";
import { IS_MAC } from "../keyboard";
import { findPluginShortcut, type PluginShortcutTarget } from "../plugin-shortcuts";

const modKey = IS_MAC ? "metaKey" : "ctrlKey";

const plugin = {
  id: "github",
  name: "GitHub",
  version: "0.1.0",
  host_api_version: "planeai.plugin-host.v1",
  source_kind: "local" as const,
  backend_entrypoint: "bin/github",
  capabilities: [],
  ui_contributions: [],
  installed_hash: null,
  installed_path: null,
  original_display_path: null,
  enabled: true,
  state: "running" as const,
  last_error: null,
  log_path: null,
};

const sessionPanel: PluginShortcutTarget = {
  plugin,
  contribution: {
    id: "pull-request",
    label: "Pull Request",
    placement: "session.panel",
    entrypoint: "ui/entry.js",
    order: null,
    shortcut: "Mod+Shift+P",
  },
};

const mainPane: PluginShortcutTarget = {
  plugin: { ...plugin, id: "fixture" },
  contribution: {
    id: "dashboard",
    label: "Dashboard",
    placement: "main-pane",
    entrypoint: "ui/dashboard.js",
    order: null,
    shortcut: "Mod+Shift+P",
  },
};

function key(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    code: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("findPluginShortcut", () => {
  it("selects a session-panel plugin for Mod+Shift+P before a legacy action can run", () => {
    expect(findPluginShortcut(key({ key: "p", code: "KeyP", [modKey]: true, shiftKey: true }), [sessionPanel], [])).toBe(sessionPanel);
  });

  it("gives a selected-session panel precedence over a main-pane shortcut", () => {
    expect(findPluginShortcut(key({ key: "p", code: "KeyP", [modKey]: true, shiftKey: true }), [sessionPanel], [mainPane])).toBe(sessionPanel);
  });

  it("does not treat unmodified or mixed-control chords as declared plugin shortcuts", () => {
    expect(findPluginShortcut(key({ key: "p", code: "KeyP", shiftKey: true }), [sessionPanel], [])).toBeNull();
    const otherModifier = IS_MAC ? "ctrlKey" : "metaKey";
    expect(findPluginShortcut(key({ key: "p", code: "KeyP", [modKey]: true, [otherModifier]: true }), [sessionPanel], [])).toBeNull();
  });
});
