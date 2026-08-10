import { describe, expect, it } from "vitest";
// Pierre does not export this manager publicly; use its installed package path to
// exercise the same vanilla/shadow-DOM integration used by CodeView.
import { InteractionManager } from "../../../node_modules/@pierre/diffs/dist/managers/InteractionManager.js";

function appendDiffLine(pre: HTMLPreElement, lineNumber: number) {
  const code = document.createElement("code");
  code.setAttribute("data-code", "");
  code.setAttribute("data-additions", "");

  const gutter = document.createElement("div");
  gutter.setAttribute("data-gutter", "");
  const number = document.createElement("div");
  number.setAttribute("data-column-number", String(lineNumber));
  number.setAttribute("data-line-index", `${lineNumber - 1},${lineNumber - 1}`);
  number.setAttribute("data-line-type", "change-addition");
  gutter.append(number);

  const content = document.createElement("div");
  const line = document.createElement("div");
  line.setAttribute("data-line", String(lineNumber));
  line.setAttribute("data-line-index", `${lineNumber - 1},${lineNumber - 1}`);
  line.setAttribute("data-line-type", "change-addition");
  content.append(line);

  code.append(gutter, content);
  pre.append(code);
  return number;
}

describe("Pierre native gutter utility", () => {
  it("places its built-in plus button on the selected range endpoint", () => {
    const pre = document.createElement("pre");
    pre.setAttribute("data-diff-type", "single");
    const endpointNumber = appendDiffLine(pre, 7);
    document.body.append(pre);

    const interactionManager = new InteractionManager("diff", {
      enableGutterUtility: true,
      onGutterUtilityClick: () => {},
    });
    interactionManager.setup(pre);
    interactionManager.setSelection({ start: 7, end: 7, side: "additions" });

    expect(endpointNumber.querySelector("[data-gutter-utility-slot]")).not.toBeNull();
    expect(endpointNumber.querySelector("button[data-utility-button]")).not.toBeNull();

    interactionManager.cleanUp();
    pre.remove();
  });
});
