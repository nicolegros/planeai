import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  getCollapsedSections,
  getLayoutWidth,
  setCollapsedSections,
  setLayoutWidth,
} from "../layout-state";

describe("layout-state", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("returns default width when nothing is stored", () => {
    expect(getLayoutWidth("sidebar", 224)).toBe(224);
  });

  it("returns stored width after setLayoutWidth", () => {
    setLayoutWidth("sidebar", 300);
    expect(getLayoutWidth("sidebar", 224)).toBe(300);
  });

  it("returns default for invalid stored value", () => {
    localStorage.setItem("planeai:layout:sidebar", "not-a-number");
    expect(getLayoutWidth("sidebar", 224)).toBe(224);
  });
});

const KEY = "planeai:layout:sidebar-collapsed";

describe("sidebar collapse state persistence", () => {
  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("returns an empty map when nothing is stored", () => {
    expect(getCollapsedSections()).toEqual({});
  });

  it("round-trips explicitly toggled sections", () => {
    setCollapsedSections({ "status:done": false, "project:alpha": true });
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({
      "status:done": false,
      "project:alpha": true,
    });
    expect(getCollapsedSections()).toEqual({ "status:done": false, "project:alpha": true });
  });

  it("ignores corrupt or non-boolean entries", () => {
    localStorage.setItem(KEY, "{not json");
    expect(getCollapsedSections()).toEqual({});
    localStorage.setItem(KEY, JSON.stringify({ a: true, b: "yes", c: 1 }));
    expect(getCollapsedSections()).toEqual({ a: true });
    localStorage.setItem(KEY, JSON.stringify([true]));
    expect(getCollapsedSections()).toEqual({});
  });

  it("survives storage being unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(getCollapsedSections()).toEqual({});
    expect(() => setCollapsedSections({ a: true })).not.toThrow();
  });
});
