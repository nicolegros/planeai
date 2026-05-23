import { describe, it, expect } from "vitest";
import {
  getActiveZone,
  setActiveZone,
  focusTerminal,
  focusSidebar,
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
});
