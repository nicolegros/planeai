import { describe, expect, it } from "vitest";
import { dropPositionToViewport, droppedPathsText } from "../dropped-paths";

const posix = { windows: false };
const windows = { windows: true };

describe("droppedPathsText", () => {
  it("types plain paths as they are, followed by a space", () => {
    expect(droppedPathsText(["/Users/me/src/main.rs"], posix)).toEqual({
      text: "/Users/me/src/main.rs ",
      skipped: [],
    });
  });

  it("escapes shell metacharacters and keeps non-ASCII letters", () => {
    expect(droppedPathsText(["/tmp/My File (1) & 'café'.png"], posix).text).toBe(
      "/tmp/My\\ File\\ \\(1\\)\\ \\&\\ \\'café\\'.png ",
    );
  });

  it("separates several paths with spaces", () => {
    expect(droppedPathsText(["/a b", "/c"], posix).text).toBe("/a\\ b /c ");
  });

  it("never types a path containing a control character", () => {
    // Ctrl-U then a newline would clear the line and run what follows.
    const hostile = "/tmp/a\x15touch pwned\n";
    expect(droppedPathsText([hostile, "/ok"], posix)).toEqual({ text: "/ok ", skipped: [hostile] });
    expect(droppedPathsText(["/tmp/\x1b[A", "/tmp/\u009b"], posix)).toEqual({
      text: "",
      skipped: ["/tmp/\x1b[A", "/tmp/\u009b"],
    });
  });

  it("double-quotes Windows paths and skips ones it cannot quote", () => {
    const unquotable = ['C:\\a"b', "C:\\%PATH%.txt", "C:\\$(calc).txt", "C:\\a`b"];
    expect(droppedPathsText(["C:\\Users\\me\\My File.txt", ...unquotable], windows)).toEqual({
      text: '"C:\\Users\\me\\My File.txt" ',
      skipped: unquotable,
    });
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
