import { describe, it, expect, beforeEach } from "vitest";
import { getLayoutWidth, setLayoutWidth } from "../layout-state";

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
