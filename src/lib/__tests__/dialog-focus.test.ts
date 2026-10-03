import { afterEach, describe, expect, it } from "vitest";
import { isInDialog } from "../dialog-focus";

afterEach(() => document.body.replaceChildren());

function inside(markup: string): Element {
  document.body.innerHTML = markup;
  return document.querySelector("#target")!;
}

describe("isInDialog", () => {
  it("recognizes ARIA dialogs, alert dialogs and open native dialogs", () => {
    expect(isInDialog(inside(`<div role="dialog"><button id="target"></button></div>`))).toBe(true);
    expect(isInDialog(inside(`<div role="alertdialog"><button id="target"></button></div>`))).toBe(
      true,
    );
    expect(isInDialog(inside(`<dialog open><button id="target"></button></dialog>`))).toBe(true);
  });

  it("is false for the page and for non-elements", () => {
    expect(isInDialog(inside(`<main><button id="target"></button></main>`))).toBe(false);
    expect(isInDialog(document.body)).toBe(false);
    expect(isInDialog(window)).toBe(false);
    expect(isInDialog(null)).toBe(false);
  });
});
