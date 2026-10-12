import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick, unmount } from "svelte";

const mocks = vi.hoisted(() => ({ refocusTerminal: vi.fn() }));
vi.mock("../../lib/focus.svelte", () => ({ refocusTerminal: mocks.refocusTerminal }));

import ToastHost, { focusNewestToast, toggleToastFocus } from "../ToastHost.svelte";
import { _resetForTests, getToasts, showToast } from "../../lib/toasts.svelte";
import { MOD_LABEL } from "../../lib/keyboard";

let component: ReturnType<typeof mount> | undefined;
let outside: HTMLButtonElement;
const writeText = vi.fn(() => Promise.resolve());

function render(props: { onboarding?: boolean } = {}) {
  const target = document.body.appendChild(document.createElement("div"));
  component = mount(ToastHost, { target, props });
  flushSync();
}

function toastEls(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>("[role='alertdialog']"));
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  (document.activeElement ?? document.body).dispatchEvent(event);
  flushSync();
  return event;
}

describe("ToastHost", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    _resetForTests();
    mocks.refocusTerminal.mockReset();
    writeText.mockClear();
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    outside = document.body.appendChild(document.createElement("button"));
    outside.textContent = "terminal";
    outside.focus();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    _resetForTests();
    document.body.replaceChildren();
    vi.useRealTimers();
  });

  it("stacks toasts oldest first and badges only the newest with the shortcut", () => {
    render();
    showToast("first");
    showToast("second");
    flushSync();

    const [first, second] = toastEls();
    expect(first.textContent).toContain("first");
    expect(second.textContent).toContain("second");
    expect(first.textContent).not.toContain(`${MOD_LABEL}U`);
    expect(second.textContent).toContain(`${MOD_LABEL}U`);
    expect(document.body.textContent).not.toContain("Click to dismiss");
  });

  it("shows a repeat count", () => {
    render();
    showToast("again");
    showToast("again");
    flushSync();
    expect(toastEls()[0].textContent).toContain("x 2");
  });

  it("returns false when there is nothing to focus", () => {
    render();
    expect(focusNewestToast()).toBe(false);
    expect(document.activeElement).toBe(outside);
  });

  it("focuses the newest toast and moves between toasts with the arrow keys", () => {
    render();
    showToast("first");
    showToast("second");
    flushSync();

    expect(focusNewestToast()).toBe(true);
    flushSync();
    const [first, second] = toastEls();
    expect(document.activeElement).toBe(second);
    expect(second.textContent).toContain("D dismiss · C copy");

    press("ArrowUp");
    expect(document.activeElement).toBe(first);
    press("ArrowUp");
    expect(document.activeElement).toBe(first);
    press("ArrowDown");
    expect(document.activeElement).toBe(second);
  });

  it("dismisses with d and moves focus to the remaining toast, then to the terminal", async () => {
    render();
    showToast("first");
    showToast("second");
    flushSync();
    focusNewestToast();

    press("d");
    await tick();
    expect(getToasts().map((toast) => toast.message)).toEqual(["first"]);
    expect(document.activeElement).toBe(toastEls()[0]);

    press("d");
    await tick();
    expect(getToasts()).toHaveLength(0);
    expect(mocks.refocusTerminal).toHaveBeenCalledOnce();
  });

  it("copies the message with c without dismissing", () => {
    render();
    showToast("copy me");
    flushSync();
    focusNewestToast();

    press("c");
    expect(writeText).toHaveBeenCalledWith("copy me");
    expect(getToasts()).toHaveLength(1);
  });

  it("leaves with Escape without dismissing and restores the previous focus", () => {
    render();
    showToast("stay");
    flushSync();
    focusNewestToast();

    const event = press("Escape");
    expect(event.defaultPrevented).toBe(true);
    expect(getToasts()).toHaveLength(1);
    expect(document.activeElement).toBe(outside);
  });

  it("toggles focus back to where it was on Mod+U", () => {
    render();
    showToast("stay");
    flushSync();

    expect(toggleToastFocus()).toBe(true);
    expect(document.activeElement).toBe(toastEls()[0]);
    expect(press("u", { metaKey: true }).defaultPrevented).toBe(false);
    expect(toggleToastFocus()).toBe(true);
    expect(document.activeElement).toBe(outside);
  });

  it("runs the action bound to a key and removes the toast", async () => {
    render();
    const run = vi.fn();
    showToast({
      message: "Update available",
      kind: "info",
      actions: [{ key: "i", label: "Install & Restart", run }],
    });
    flushSync();
    focusNewestToast();
    flushSync();
    expect(toastEls()[0].textContent).toContain("I install & restart · D dismiss · C copy");

    press("I", { shiftKey: true });
    await tick();
    expect(run).toHaveBeenCalledOnce();
    expect(getToasts()).toHaveLength(0);
    expect(mocks.refocusTerminal).toHaveBeenCalledOnce();
  });

  it("dismisses on body click and runs actions on button click", () => {
    render();
    const run = vi.fn();
    showToast("plain");
    showToast({ message: "prompt", actions: [{ key: "a", label: "Archive", run }] });
    flushSync();

    toastEls()[1].querySelector("button")!.click();
    flushSync();
    expect(run).toHaveBeenCalledOnce();
    expect(getToasts().map((toast) => toast.message)).toEqual(["plain"]);

    toastEls()[0].click();
    flushSync();
    expect(getToasts()).toHaveLength(0);
    expect(writeText).not.toHaveBeenCalled();
  });

  it("keeps a toast whose dismiss is disabled", () => {
    render();
    showToast({ message: "Installing", kind: "info", dismiss: "disabled" });
    flushSync();
    focusNewestToast();
    flushSync();

    press("d");
    toastEls()[0].click();
    flushSync();
    expect(getToasts()).toHaveLength(1);
    expect(toastEls()[0].textContent).not.toContain("D dismiss");
  });

  it("pauses the countdown while the toast is focused", () => {
    render();
    const onExpire = vi.fn();
    showToast({
      message: "PR merged",
      kind: "success",
      countdown: { seconds: 30, label: (remaining) => `auto-archives in ${remaining}s`, onExpire },
    });
    flushSync();
    vi.advanceTimersByTime(2_000);
    flushSync();
    expect(toastEls()[0].textContent).toContain("auto-archives in 28s");

    focusNewestToast();
    vi.advanceTimersByTime(60_000);
    flushSync();
    expect(onExpire).not.toHaveBeenCalled();
    expect(toastEls()[0].textContent).toContain("auto-archives in 28s");

    press("Escape");
    vi.advanceTimersByTime(28_000);
    expect(onExpire).toHaveBeenCalledOnce();
  });

  it("hides onboarding-hidden toasts during onboarding", () => {
    render({ onboarding: true });
    showToast({ message: "Update available", kind: "info", hiddenDuringOnboarding: true });
    showToast("visible");
    flushSync();

    expect(toastEls().map((element) => element.textContent)).toEqual([
      expect.stringContaining("visible"),
    ]);
  });

  it("leaves keyboard focus in place when a toast is clicked", () => {
    render();
    showToast("first");
    showToast("second");
    flushSync();

    const mousedown = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    toastEls()[1].dispatchEvent(mousedown);
    toastEls()[1].click();
    flushSync();

    expect(mousedown.defaultPrevented).toBe(true);
    expect(toastEls()).toHaveLength(1);
    expect(document.activeElement).toBe(outside);
  });

  it("puts the shortcut keycap on the message row, without a footer line", () => {
    render();
    showToast("plain");
    flushSync();

    const [toast] = toastEls();
    expect(toast.querySelector("kbd")?.textContent).toBe(`${MOD_LABEL}U`);
    expect(toast.querySelectorAll("p")).toHaveLength(1);

    focusNewestToast();
    flushSync();
    expect(toast.querySelector("kbd")?.textContent).toBe("Esc");
  });
});
