import { describe, expect, it } from "vitest";
import { dropPositionToViewport, droppedPathsText } from "../dropped-paths";

describe("droppedPathsText", () => {
  it("types plain paths as they are, followed by a space", () => {
    expect(droppedPathsText(["/Users/me/src/main.rs"])).toBe("/Users/me/src/main.rs ");
  });

  it("escapes shell metacharacters and keeps non-ASCII letters", () => {
    expect(droppedPathsText(["/tmp/My File (1) & 'café'.png"])).toBe(
      "/tmp/My\\ File\\ \\(1\\)\\ \\&\\ \\'café\\'.png ",
    );
  });

  it("separates several paths with spaces", () => {
    expect(droppedPathsText(["/a b", "/c"])).toBe("/a\\ b /c ");
  });

  it("single-quotes a path containing a control character", () => {
    expect(droppedPathsText(["/tmp/a\nb'c"])).toBe("'/tmp/a\nb'\\''c' ");
  });
});

describe("dropPositionToViewport", () => {
  it("keeps logical points from WKWebView and WebKitGTK", () => {
    expect(dropPositionToViewport({ x: 300, y: 120 }, { windows: false, pixelRatio: 2 })).toEqual({
      x: 300,
      y: 120,
    });
  });

  it("scales WebView2's physical pixels down to the viewport", () => {
    expect(dropPositionToViewport({ x: 300, y: 120 }, { windows: true, pixelRatio: 1.5 })).toEqual({
      x: 200,
      y: 80,
    });
  });
});
