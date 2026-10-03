import { describe, expect, it } from "vitest";
import { IS_MAC } from "../keyboard";
import {
  findPluginShortcut,
  isTextEditingChord,
  type PluginShortcutTarget,
} from "../plugin-shortcuts";

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
  providers: [],
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
  it("selects a session-panel plugin for its declared Mod+Shift+P shortcut", () => {
    expect(
      findPluginShortcut(
        key({ key: "p", code: "KeyP", [modKey]: true, shiftKey: true }),
        [sessionPanel],
        [],
      ),
    ).toBe(sessionPanel);
  });

  it("gives a selected-session panel precedence over a main-pane shortcut", () => {
    expect(
      findPluginShortcut(
        key({ key: "p", code: "KeyP", [modKey]: true, shiftKey: true }),
        [sessionPanel],
        [mainPane],
      ),
    ).toBe(sessionPanel);
  });

  it("does not treat unmodified or mixed-control chords as declared plugin shortcuts", () => {
    expect(
      findPluginShortcut(key({ key: "p", code: "KeyP", shiftKey: true }), [sessionPanel], []),
    ).toBeNull();
    const otherModifier = IS_MAC ? "ctrlKey" : "metaKey";
    expect(
      findPluginShortcut(
        key({ key: "p", code: "KeyP", [modKey]: true, [otherModifier]: true }),
        [sessionPanel],
        [],
      ),
    ).toBeNull();
  });
});

describe("isTextEditingChord", () => {
  const chord = (key: string, extra: Partial<KeyboardEvent> = {}) => ({
    key,
    metaKey: true,
    ctrlKey: false,
    altKey: false,
    ...extra,
  });

  it("recognizes the chords a text field handles itself", () => {
    for (const key of [
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
      "ArrowDown",
      "Backspace",
      "Delete",
      "a",
      "c",
      "v",
      "x",
      "z",
      "Z",
      "y",
    ]) {
      expect(isTextEditingChord(chord(key)), key).toBe(true);
    }
    expect(isTextEditingChord(chord("ArrowLeft", { ctrlKey: true, metaKey: false }))).toBe(true);
  });

  it("leaves app chords, and chords without a modifier, to the app", () => {
    expect(isTextEditingChord(chord("["))).toBe(false);
    expect(isTextEditingChord(chord("n"))).toBe(false);
    // Mod+Alt+Arrow moves tabs between panes.
    expect(isTextEditingChord(chord("ArrowLeft", { altKey: true }))).toBe(false);
    expect(isTextEditingChord(chord("ArrowLeft", { metaKey: false }))).toBe(false);
  });

  it("stays self-contained, since plugin frames inline its source", () => {
    const inlined = new Function(
      `return (${isTextEditingChord.toString()})`,
    )() as typeof isTextEditingChord;
    expect(inlined(chord("ArrowUp"))).toBe(true);
  });
});
