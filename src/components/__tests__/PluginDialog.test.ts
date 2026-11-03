import { afterAll, afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import { stubLayoutAsVisible } from "./dom-visibility";

vi.mock("../../lib/api", () => ({
  plugins: {
    call: vi.fn(() => Promise.resolve(null)),
    hostCall: vi.fn(() => Promise.resolve(null)),
    localUiSource: vi.fn(() => new Promise(() => {})),
    settings: vi.fn(() => Promise.resolve({})),
    updateSettings: vi.fn((_: string, settings: unknown) => Promise.resolve(settings)),
    dataChanged: vi.fn(() => Promise.resolve()),
  },
}));
vi.mock("../../lib/settings.svelte", () => ({ isDark: () => false }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
const resizeCallbacks = new Set<() => void>();
vi.stubGlobal(
  "ResizeObserver",
  class {
    constructor(private callback: () => void) {}
    observe() {
      resizeCallbacks.add(this.callback);
    }
    disconnect() {
      resizeCallbacks.delete(this.callback);
    }
  },
);
vi.stubGlobal(
  "fetch",
  vi.fn(async () => new Response(new Uint8Array())),
);

import PluginDialogHarness from "./PluginDialogHarness.svelte";
import { FRAME_INTERACTION_EVENT } from "../../lib/frame-interaction";

const restoreLayout = stubLayoutAsVisible();
afterAll(restoreLayout);

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = "";
});

function open(placement: "dialog" | "session.panel"): HTMLElement {
  component = mount(PluginDialogHarness, { target: document.body, props: { placement } });
  flushSync();
  return document.querySelector<HTMLElement>("[role=dialog]")!;
}

describe("PluginDialog", () => {
  it("shows a dialog contribution in the wide host dialog titled by its label, without a session", async () => {
    const dialog = open("dialog");
    expect(dialog.textContent).toContain("Routines");
    expect(dialog.className).toContain("w-[min(640px,calc(100vw-32px))]");
    expect(dialog.className).not.toContain("452px");
    expect(dialog.className).toContain("top-[7.5vh]");
    expect(dialog.className).not.toContain("-translate-y-1/2");
    await vi.waitFor(() =>
      expect(
        dialog.querySelector("[data-plugin-ui-contribution]")?.shadowRoot?.querySelector("iframe")
          ?.title,
      ).toBe("Routines"),
    );
  });

  it("closes a dialog contribution from its close button", () => {
    open("dialog");
    document.querySelector<HTMLButtonElement>('button[aria-label="Close Routines"]')!.click();
    flushSync();
    expect(document.querySelector("[role=dialog]")).toBeNull();
  });

  it("closes on Escape", async () => {
    open("dialog");
    document
      .querySelector("[role=dialog]")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await vi.waitFor(() => expect(document.querySelector("[role=dialog]")).toBeNull());
  });

  it("opens a session panel centered in the form-width dialog, without a close button", () => {
    const dialog = open("session.panel");
    expect(dialog.className).toContain("w-[min(452px,calc(100vw-32px))]");
    expect(dialog.className).toContain("min-h-[min(360px,85vh)]");
    expect(dialog.className).toContain("top-1/2 -translate-y-1/2");
    expect(document.querySelector('button[aria-label="Close Routines"]')).toBeNull();
  });

  it("holds a session panel's top edge once the user acts inside its plugin frame", () => {
    const dialog = open("session.panel");
    vi.spyOn(dialog, "getBoundingClientRect").mockReturnValue({ top: 180 } as DOMRect);

    document
      .querySelector("[data-plugin-ui-contribution]")!
      .dispatchEvent(new CustomEvent(FRAME_INTERACTION_EVENT, { bubbles: true }));
    flushSync();

    expect(
      dialog.className,
      "content that changes height now grows the bottom edge only",
    ).not.toContain("top-1/2");
    expect(dialog.style.top).toBe("180px");
  });

  it("moves a held session panel up only as far as grown content needs to stay inside the window", () => {
    const dialog = open("session.panel");
    vi.spyOn(dialog, "getBoundingClientRect").mockReturnValue({ top: 180 } as DOMRect);
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "j", bubbles: true }));
    Object.defineProperty(dialog, "offsetHeight", { configurable: true, value: 640 });

    for (const callback of resizeCallbacks) callback();
    flushSync();

    expect(window.innerHeight).toBe(768);
    expect(dialog.style.top, "bottom edge stays 7.5vh above the window's").toBe(
      `${768 * 0.925 - 640}px`,
    );
  });
});
