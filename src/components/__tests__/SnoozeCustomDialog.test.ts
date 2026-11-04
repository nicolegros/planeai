import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { flushFrames, stubLayoutAsVisible } from "./dom-visibility";
import SnoozeCustomDialog from "../SnoozeCustomDialog.svelte";

const restoreLayoutStubs = stubLayoutAsVisible();
afterAll(restoreLayoutStubs);

let dialog: Record<string, unknown> | null = null;

afterEach(() => {
  if (dialog) unmount(dialog, { outro: false });
  dialog = null;
  document.body.innerHTML = "";
});

async function openDialog(): Promise<HTMLElement> {
  dialog = mount(SnoozeCustomDialog, {
    target: document.body,
    props: { title: "Snooze PLA-1", onSnooze: vi.fn(async () => true), onCancel: vi.fn() },
  }) as Record<string, unknown>;
  flushSync();
  await flushFrames();
  return document.querySelector<HTMLElement>("[data-form-keyboard]")!;
}

function press(target: HTMLElement, key: string, init: KeyboardEventInit = {}): void {
  target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  flushSync();
}

const fieldLabels = () =>
  [...document.querySelectorAll("[data-field]")].map((field) => field.getAttribute("data-field"));

describe("SnoozeCustomDialog", () => {
  it("switches between a time and an amount with m, both ways", async () => {
    const form = await openDialog();
    expect(fieldLabels()).toEqual(["date", "time"]);

    press(form, "m");
    expect(fieldLabels()).toEqual(["amount", "unit"]);

    press(form, "m");
    expect(fieldLabels()).toEqual(["date", "time"]);

    press(form, "M", { shiftKey: true });
    expect(fieldLabels()).toEqual(["amount", "unit"]);
  });
});
