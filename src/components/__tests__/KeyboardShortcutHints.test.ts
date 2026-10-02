import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const layout = vi.hoisted(() => ({ focusedTabType: null as string | null }));
vi.mock("../../lib/task-workspace-layout.svelte", () => ({
  taskWorkspaceLayout: {
    focusedTab: () => (layout.focusedTabType ? { type: layout.focusedTabType } : null),
  },
}));

import KeyboardHelperBar from "../KeyboardHelperBar.svelte";
import KeyboardShortcuts from "../KeyboardShortcuts.svelte";
import { focusEditor, focusTerminal } from "../../lib/focus.svelte";
import { MOD_ENTER_HINT, MOD_LABEL } from "../../lib/keyboard";

describe("editor feedback shortcut hints", () => {
  let components: ReturnType<typeof mount>[] = [];
  let targets: HTMLElement[] = [];

  beforeEach(() => {
    vi.useFakeTimers();
  });

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

  afterEach(async () => {
    for (const component of components) unmount(component);
    await tick();
    vi.runAllTimers();
    for (const target of targets) target.remove();
    components = [];
    targets = [];
    layout.focusedTabType = null;
    focusTerminal();
    vi.useRealTimers();
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

  it("shows diff navigation hints while the focused pane shows a diff", async () => {
    layout.focusedTabType = "diff";
    focusTerminal();
    const target = mountHelperBar();
    await tick();

    expect(target.textContent).toContain("Hunk");
    expect(target.textContent).toContain("Unified/Split");
  });

  it("always points to the full shortcut list, in every zone", async () => {
    for (const zone of [focusTerminal, focusEditor]) {
      zone();
      const target = mountHelperBar();
      await tick();
      const allShortcuts = target.querySelector("[data-helper-all-shortcuts]");
      expect(allShortcuts?.textContent?.replace(/\s+/g, " ").trim()).toBe(
        `${MOD_LABEL}/All shortcuts`,
      );
      // Outside the row that drops hints on a narrow window.
      expect(target.querySelector("[data-helper-hints]")?.contains(allShortcuts!)).toBe(false);
    }
  });

  it("hides hints that do not fit whole instead of clipping them", async () => {
    focusTerminal();
    const row = mountHelperBar().querySelector("[data-helper-hints]")!;
    await tick();
    expect(row.className).toContain("flex-wrap");
    expect(row.className).toContain("overflow-hidden");
  });

  it("lists the feedback send shortcut in the editor shortcut reference", async () => {
    focusEditor();
    mountShortcutDialog();
    await tick();

    expect(document.body.textContent).toContain("Editor");
    expect(document.body.textContent).toContain(`${MOD_LABEL}⇧C`);
    expect(document.body.textContent).toContain("Comment on selected code");
    expect(document.body.textContent).toContain(MOD_ENTER_HINT);
    expect(document.body.textContent).toContain("Send queued editor feedback");
  });
});
