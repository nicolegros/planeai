import { describe, it, expect } from "vitest";
import { getMruList, touchMru, removeMru } from "../mru.svelte";

describe("MRU list", () => {
  it("starts empty", () => {
    // Clear state
    for (const id of getMruList()) removeMru(id);
    expect(getMruList()).toEqual([]);
  });

  it("touchMru adds to front", () => {
    for (const id of getMruList()) removeMru(id);
    touchMru("a");
    touchMru("b");
    touchMru("c");
    expect(getMruList()).toEqual(["c", "b", "a"]);
  });

  it("touchMru moves existing to front", () => {
    for (const id of getMruList()) removeMru(id);
    touchMru("a");
    touchMru("b");
    touchMru("c");
    touchMru("a");
    expect(getMruList()).toEqual(["a", "c", "b"]);
  });

  it("removeMru removes from list", () => {
    for (const id of getMruList()) removeMru(id);
    touchMru("a");
    touchMru("b");
    removeMru("a");
    expect(getMruList()).toEqual(["b"]);
  });
});
