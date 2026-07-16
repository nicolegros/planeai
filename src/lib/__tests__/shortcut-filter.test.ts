import { describe, it, expect } from "vitest";
import { filterShortcuts, type ShortcutSection } from "../shortcut-filter";

const sections: ShortcutSection[] = [
  {
    section: "General",
    items: [
      { keys: "⌘K", description: "Command menu" },
      { keys: "⌘/", description: "Keyboard shortcuts" },
      { keys: "Escape", description: "Dismiss / focus terminal" },
    ],
  },
  {
    section: "Sessions",
    items: [
      { keys: "⌘N", description: "New session" },
      { keys: "⌘⇧N", description: "New project" },
    ],
  },
  {
    section: "Tabs",
    items: [
      { keys: "⌘T", description: "New tab" },
      { keys: "⌘W", description: "Close tab" },
    ],
  },
];

describe("filterShortcuts", () => {
  it("returns all sections unchanged when query is empty", () => {
    expect(filterShortcuts(sections, "")).toEqual(sections);
    expect(filterShortcuts(sections, "   ")).toEqual(sections);
  });

  it("returns a flat list of matching items (case-insensitive substring)", () => {
    const result = filterShortcuts(sections, "new");
    expect(result).toHaveLength(1);
    expect(result[0].section).toBe("");
    expect(result[0].items).toEqual([
      { keys: "⌘N", description: "New session" },
      { keys: "⌘⇧N", description: "New project" },
      { keys: "⌘T", description: "New tab" },
    ]);
  });

  it("matches case-insensitively", () => {
    const result = filterShortcuts(sections, "COMMAND");
    expect(result[0].items).toEqual([
      { keys: "⌘K", description: "Command menu" },
    ]);
  });

  it("returns empty array when nothing matches", () => {
    const result = filterShortcuts(sections, "zzzzz");
    expect(result).toEqual([]);
  });

  it("matches partial substrings", () => {
    const result = filterShortcuts(sections, "tab");
    expect(result[0].items).toHaveLength(2);
    expect(result[0].items[0].description).toBe("New tab");
    expect(result[0].items[1].description).toBe("Close tab");
  });

  it("trims the query before matching", () => {
    const result = filterShortcuts(sections, "  command  ");
    expect(result[0].items).toEqual([
      { keys: "⌘K", description: "Command menu" },
    ]);
  });
});
