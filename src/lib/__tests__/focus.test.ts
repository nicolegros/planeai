import { describe, it, expect } from "vitest";
import {
  getActiveZone,
  getExplorerReturnZone,
  setActiveZone,
  focusTerminal,
  focusEditor,
  focusSidebar,
  toggleExplorerFocus,
  toggleSidebar,
} from "../focus.svelte";

describe("focus zone state", () => {
  it("defaults to terminal", () => {
    // Reset to known state
    focusTerminal();
    expect(getActiveZone()).toBe("terminal");
  });

  it("setActiveZone changes the zone", () => {
    setActiveZone("sidebar");
    expect(getActiveZone()).toBe("sidebar");
    setActiveZone("terminal");
    expect(getActiveZone()).toBe("terminal");
  });

  it("focusSidebar sets zone to sidebar", () => {
    focusTerminal();
    focusSidebar();
    expect(getActiveZone()).toBe("sidebar");
  });

  it("focusTerminal sets zone to terminal", () => {
    focusSidebar();
    focusTerminal();
    expect(getActiveZone()).toBe("terminal");
  });

  it("toggleSidebar toggles between terminal and sidebar", () => {
    focusTerminal();
    toggleSidebar();
    expect(getActiveZone()).toBe("sidebar");
    toggleSidebar();
    expect(getActiveZone()).toBe("terminal");
  });

  it("toggleExplorerFocus restores the editor origin", () => {
    focusEditor();
    toggleExplorerFocus();
    expect(getActiveZone()).toBe("explorer");
    expect(getExplorerReturnZone()).toBe("editor");
    toggleExplorerFocus();
    expect(getActiveZone()).toBe("editor");
  });

  it("toggleExplorerFocus switches between Explorer and terminal", () => {
    focusTerminal();
    toggleExplorerFocus();
    expect(getActiveZone()).toBe("explorer");
    toggleExplorerFocus();
    expect(getActiveZone()).toBe("terminal");
  });
});
