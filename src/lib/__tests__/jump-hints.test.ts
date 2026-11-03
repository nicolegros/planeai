import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createModifierHold, JUMP_HINT_SHOW_DELAY_MS, jumpHintLabels } from "../jump-hints.svelte";

function keydown(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, ...init });
}

const cmd = () => keydown("Meta", { metaKey: true });

describe("createModifierHold", () => {
  let shown: boolean[];
  let hold: ReturnType<typeof createModifierHold>;

  beforeEach(() => {
    vi.useFakeTimers();
    shown = [];
    hold = createModifierHold("Meta", (visible) => shown.push(visible));
  });

  afterEach(() => {
    hold.reset();
    vi.useRealTimers();
  });

  it("shows after the modifier is held alone for the full delay, not a millisecond earlier", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS - 1);
    expect(shown).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(shown).toEqual([true]);
    expect(JUMP_HINT_SHOW_DELAY_MS).toBe(200);
  });

  it("never shows for a quick shortcut such as Cmd+C", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(50);
    hold.keydown(keydown("c", { metaKey: true }));
    hold.keyup();
    vi.advanceTimersByTime(1000);
    expect(shown).toEqual([]);
  });

  it("does not start while another modifier is down", () => {
    hold.keydown(keydown("Meta", { metaKey: true, shiftKey: true }));
    hold.keydown(keydown("Meta", { metaKey: true, altKey: true }));
    hold.keydown(keydown("Meta", { metaKey: true, ctrlKey: true }));
    hold.keydown(keydown("Control", { ctrlKey: true }));
    vi.advanceTimersByTime(1000);
    expect(shown).toEqual([]);
  });

  it("hides on release", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS);
    hold.keyup();
    expect(shown).toEqual([true, false]);
  });

  it("hides when another key goes down, a modifier joining included", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS);
    hold.keydown(keydown("1", { metaKey: true }));
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS);
    hold.keydown(keydown("Shift", { metaKey: true, shiftKey: true }));
    expect(shown).toEqual([true, false, true, false]);
  });

  it("hides on reset, which blur and a hidden document trigger", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS);
    hold.reset();
    hold.keydown(cmd());
    hold.reset();
    vi.advanceTimersByTime(1000);
    expect(shown).toEqual([true, false]);
  });

  it("keeps showing through key repeat and restarts the delay on a fresh hold", () => {
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS);
    hold.keydown(keydown("Meta", { metaKey: true, repeat: true }));
    hold.keyup();
    hold.keydown(cmd());
    vi.advanceTimersByTime(JUMP_HINT_SHOW_DELAY_MS - 1);
    expect(shown).toEqual([true, false]);
    vi.advanceTimersByTime(1);
    expect(shown).toEqual([true, false, true]);
  });
});

describe("jumpHintLabels", () => {
  it("labels the first nine targets in order and leaves the rest out", () => {
    const order = Array.from({ length: 10 }, (_, i) => `task:p:T-${i + 1}`);
    expect([...jumpHintLabels(order, "⌘")]).toEqual([
      ["task:p:T-1", "⌘1"],
      ["task:p:T-2", "⌘2"],
      ["task:p:T-3", "⌘3"],
      ["task:p:T-4", "⌘4"],
      ["task:p:T-5", "⌘5"],
      ["task:p:T-6", "⌘6"],
      ["task:p:T-7", "⌘7"],
      ["task:p:T-8", "⌘8"],
      ["task:p:T-9", "⌘9"],
    ]);
  });

  it("uses the platform label as given", () => {
    expect([...jumpHintLabels(["scratch"], "Ctrl+")]).toEqual([["scratch", "Ctrl+1"]]);
  });
});
