import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

vi.mock("../../lib/session-orchestrator.svelte", () => ({
  getActiveSessionId: () => null,
}));
vi.mock("../../lib/tab-layout.svelte", () => ({
  getDiffTabActive: () => ({}),
}));

import KeyboardHelperBar from "../KeyboardHelperBar.svelte";
import KeyboardShortcuts from "../KeyboardShortcuts.svelte";
import { focusEditor, focusTerminal } from "../../lib/focus.svelte";
import { MOD_ENTER_HINT, MOD_LABEL } from "../../lib/keyboard";

describe("editor feedback shortcut hints", () => {
  let components: ReturnType<typeof mount>[] = [];
  let targets: HTMLElement[] = [];

  function createTarget(): HTMLDivElement {
    const target = document.createElement("div");
    document.body.append(target);
    targets.push(target);
    return target;
  }

  function mountHelperBar(): HTMLDivElement {
    const target = createTarget();
    components.push(mount(KeyboardHelperBar, { target }));
    return target;
  }

  function mountShortcutDialog(): void {
    components.push(
      mount(KeyboardShortcuts, {
        target: createTarget(),
        props: { open: true, onOpenChange: vi.fn() },
      }),
    );
  }

  afterEach(() => {
    for (const component of components) unmount(component);
    for (const target of targets) target.remove();
    components = [];
    targets = [];
    focusTerminal();
  });

  it("shows the feedback send shortcut in the editor bottom helper", async () => {
    focusEditor();
    const target = mountHelperBar();
    await tick();

    expect(target.textContent).toContain(`${MOD_LABEL}⇧C`);
    expect(target.textContent).toContain("Comment");
    expect(target.textContent).toContain(MOD_ENTER_HINT);
    expect(target.textContent).toContain("Send feedback");
  });

  it("lists the feedback send shortcut in the editor shortcut reference", async () => {
    focusEditor();
    mountShortcutDialog();
    await tick();

    expect(document.body.textContent).toContain("Editor");
    expect(document.body.textContent).toContain(MOD_ENTER_HINT);
    expect(document.body.textContent).toContain("Send queued editor feedback");
  });
});
