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
vi.stubGlobal(
  "fetch",
  vi.fn(async () => new Response(new Uint8Array())),
);

import PluginDialogHarness from "./PluginDialogHarness.svelte";

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

  it("keeps a session panel in the form-width dialog without a close button", () => {
    const dialog = open("session.panel");
    expect(dialog.className).toContain("w-[min(452px,calc(100vw-32px))]");
    expect(dialog.className).toContain("min-h-[min(360px,85vh)]");
    expect(document.querySelector('button[aria-label="Close Routines"]')).toBeNull();
  });
});
