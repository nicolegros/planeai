import { describe, expect, it } from "vitest";
import { matchChord, IS_MAC } from "../keyboard";

const modKey = IS_MAC ? "metaKey" : "ctrlKey";
function key(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("retired legacy PR shortcut", () => {
  it("does not reserve Mod+Shift+P", () => {
    expect(matchChord(key({ key: "p", [modKey]: true, shiftKey: true }))).toBeNull();
  });
  it("keeps Mod+P for the file finder", () => {
    expect(matchChord(key({ key: "p", [modKey]: true }))).toEqual({ type: "open_file" });
  });
});
