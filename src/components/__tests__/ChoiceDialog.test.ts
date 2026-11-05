import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { flushFrames, stubLayoutAsVisible } from "./dom-visibility";
import ChoiceDialog, { type Choice } from "../ChoiceDialog.svelte";

const restoreLayoutStubs = stubLayoutAsVisible();
afterAll(restoreLayoutStubs);

let dialog: Record<string, unknown> | null = null;

afterEach(() => {
  if (dialog) unmount(dialog, { outro: false });
  dialog = null;
  document.body.innerHTML = "";
});

async function openDialog(options: Choice[]) {
  const onChoose = vi.fn();
  const onClose = vi.fn();
  dialog = mount(ChoiceDialog, {
    target: document.body,
    props: { title: "New tab", options, onChoose, onClose },
  }) as Record<string, unknown>;
  flushSync();
  await flushFrames();
  return { onChoose, onClose };
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  document.body.dispatchEvent(event);
  flushSync();
  return event;
}

function row(label: string): HTMLButtonElement {
  const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.textContent?.includes(label),
  );
  if (!button) throw new Error(`no row labelled ${label}`);
  return button;
}

const newTabOptions: Choice[] = [
  { id: "agent", label: "Agent session", glyph: "›_", key: "a" },
  { id: "terminal", label: "Terminal", glyph: "$", key: "t" },
];

describe("ChoiceDialog", () => {
  it("renders the title and every option label", async () => {
    await openDialog(newTabOptions);

    expect(document.body.textContent).toContain("New tab");
    expect(row("Agent session").getAttribute("aria-keyshortcuts")).toBe("a");
    expect(row("Terminal").getAttribute("aria-keyshortcuts")).toBe("t");
  });

  it("marks the highlighted option as selected and follows the arrow keys", async () => {
    await openDialog(newTabOptions);
    expect(row("Agent session").getAttribute("aria-selected")).toBe("true");
    expect(row("Terminal").getAttribute("aria-selected")).toBe("false");

    press("ArrowDown");

    expect(row("Agent session").getAttribute("aria-selected")).toBe("false");
    expect(row("Terminal").getAttribute("aria-selected")).toBe("true");
  });

  it("explains why a disabled option is unavailable", async () => {
    await openDialog([
      { id: "agent", label: "Agent session", glyph: "›_", key: "a" },
      {
        id: "terminal",
        label: "Terminal",
        glyph: "$",
        key: "t",
        disabled: true,
        disabledHint: "Open a session first",
      },
    ]);

    expect(row("Terminal").title).toBe("Open a session first");
    expect(row("Agent session").title).toBe("");
  });

  it("picks an option by its key", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    const event = press("t");

    expect(onChoose).toHaveBeenCalledWith("terminal");
    expect(event.defaultPrevented).toBe(true);
  });

  it("ignores the key of a disabled option", async () => {
    const { onChoose } = await openDialog([
      newTabOptions[0],
      { ...newTabOptions[1], disabled: true },
    ]);

    press("t");

    expect(onChoose).not.toHaveBeenCalled();
  });

  it("ignores keys pressed with a modifier", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    press("t", { metaKey: true });
    press("a", { ctrlKey: true });

    expect(onChoose).not.toHaveBeenCalled();
  });

  it("picks the first option on Enter", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("agent");
  });

  it("moves the highlight with the arrow keys", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    press("ArrowDown");
    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("terminal");
  });

  it("moves the highlight back up with ArrowUp", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    press("ArrowDown");
    press("ArrowUp");
    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("agent");
  });

  it("skips disabled options when moving the highlight", async () => {
    const { onChoose } = await openDialog([
      { id: "one", label: "One", glyph: "1", key: "o" },
      { id: "two", label: "Two", glyph: "2", key: "w", disabled: true },
      { id: "three", label: "Three", glyph: "3", key: "h" },
    ]);

    press("ArrowDown");
    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("three");
  });

  it("starts on the first enabled option", async () => {
    const { onChoose } = await openDialog([
      { ...newTabOptions[0], disabled: true },
      newTabOptions[1],
    ]);

    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("terminal");
  });

  it("closes on Escape", async () => {
    const { onChoose, onClose } = await openDialog(newTabOptions);

    press("Escape");

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onChoose).not.toHaveBeenCalled();
  });

  it("picks an option on click", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    row("Terminal").click();

    expect(onChoose).toHaveBeenCalledWith("terminal");
  });

  it("does nothing when a disabled row is clicked", async () => {
    const { onChoose } = await openDialog([
      newTabOptions[0],
      { ...newTabOptions[1], disabled: true },
    ]);

    row("Terminal").click();

    expect(onChoose).not.toHaveBeenCalled();
  });

  it("highlights the row under the pointer", async () => {
    const { onChoose } = await openDialog(newTabOptions);

    row("Terminal").dispatchEvent(new PointerEvent("pointermove", { bubbles: true }));
    flushSync();
    press("Enter");

    expect(onChoose).toHaveBeenCalledWith("terminal");
  });
});
