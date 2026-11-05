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

const onCancel = vi.fn();

async function openDialog(): Promise<HTMLElement> {
  onCancel.mockClear();
  dialog = mount(SnoozeCustomDialog, {
    target: document.body,
    props: { title: "Snooze PLA-1", onSnooze: vi.fn(async () => true), onCancel },
  }) as Record<string, unknown>;
  flushSync();
  await flushFrames();
  return document.querySelector<HTMLElement>("[data-form-keyboard]")!;
}

function press(target: HTMLElement, key: string, init: KeyboardEventInit = {}): void {
  target.dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }),
  );
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

  it.each([
    [
      "the mode radio",
      "at",
      () => document.querySelector<HTMLElement>("input[type='radio'][value='after']")!,
    ],
    [
      "the date field",
      "at",
      () => document.querySelector<HTMLElement>("[data-field='date'] input")!,
    ],
    [
      "the time field",
      "at",
      () => document.querySelector<HTMLElement>("[data-field='time'] input")!,
    ],
    [
      "the amount field",
      "after",
      () => document.querySelector<HTMLElement>("[data-field='amount'] input")!,
    ],
    [
      "the Cancel button",
      "at",
      () =>
        [...document.querySelectorAll<HTMLElement>("button")].find(
          (b) => b.textContent?.trim() === "Cancel",
        )!,
    ],
    [
      "the Snooze button",
      "at",
      () => document.querySelector<HTMLElement>("button[type='submit']")!,
    ],
  ] as const)(
    "Escape on %s unfocuses it, then a second Escape closes the dialog",
    async (_name, mode, control) => {
      const form = await openDialog();
      if (mode === "after") press(form, "m");

      const el = control();
      el.focus();
      flushSync();
      expect(document.activeElement).toBe(el);

      press(el, "Escape");
      expect(onCancel).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(form);

      press(form, "Escape");
      expect(onCancel).toHaveBeenCalledTimes(1);
    },
  );

  it("closes the unit list on the first Escape, unfocuses the select on the second", async () => {
    const form = await openDialog();
    press(form, "m");
    const unit = document.querySelector<HTMLElement>("[data-field='unit'] input")!;
    unit.focus();
    flushSync();
    expect(unit.getAttribute("aria-expanded")).toBe("true");

    press(unit, "Escape");
    expect(unit.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(unit);

    press(unit, "Escape");
    expect(onCancel).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(form);
  });
});
