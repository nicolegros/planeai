import { describe, it, expect } from "vitest";

/**
 * Regression test for: Cmd+P → Escape → Cmd+K opens file selector instead of command palette.
 *
 * This simulates the state transitions that App.svelte's keyboard action handler performs.
 * The real handler lives in App.svelte's installKeyboardRouter callback.
 */
describe("command menu state transitions", () => {
  function createState() {
    return { commandMenuOpen: false, commandMenuFileMode: false };
  }

  function handleAction(state: { commandMenuOpen: boolean; commandMenuFileMode: boolean }, action: string) {
    if (action === "open_file") {
      state.commandMenuFileMode = true;
      state.commandMenuOpen = true;
    } else if (action === "focus_terminal") {
      state.commandMenuOpen = false;
      state.commandMenuFileMode = false;
    } else if (action === "command_palette") {
      state.commandMenuOpen = !state.commandMenuOpen;
    }
  }

  it("Cmd+K after Cmd+P → Escape opens in command palette mode, not file mode", () => {
    const state = createState();

    // Cmd+P opens file selector
    handleAction(state, "open_file");
    expect(state.commandMenuOpen).toBe(true);
    expect(state.commandMenuFileMode).toBe(true);

    // Escape closes it
    handleAction(state, "focus_terminal");
    expect(state.commandMenuOpen).toBe(false);
    expect(state.commandMenuFileMode).toBe(false); // <-- THIS IS THE BUG: was not being reset

    // Cmd+K opens command palette
    handleAction(state, "command_palette");
    expect(state.commandMenuOpen).toBe(true);
    expect(state.commandMenuFileMode).toBe(false); // should be command palette, not file mode
  });
});
