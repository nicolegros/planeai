import { afterEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import SessionRenameInput from "../SessionRenameInput.svelte";

describe("SessionRenameInput", () => {
  let component: ReturnType<typeof mount> | undefined;
  let target: HTMLElement;

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  function render() {
    const onCommit = vi.fn();
    const onCancel = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(SessionRenameInput, {
      target,
      props: { value: "feature", onCommit, onCancel },
    });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input")!;
    return { input, onCommit, onCancel };
  }

  function press(input: HTMLInputElement, key: string) {
    input.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  }

  it("is labelled, shows the value and takes focus once mounted", async () => {
    const { input } = render();
    expect(input.getAttribute("aria-label")).toBe("Rename session");
    expect(input.value).toBe("feature");
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect(document.activeElement).toBe(input);
  });

  it("commits on Enter and on blur, and cancels on Escape", () => {
    const { input, onCommit, onCancel } = render();
    press(input, "Enter");
    expect(onCommit).toHaveBeenCalledTimes(1);
    press(input, "Escape");
    expect(onCancel).toHaveBeenCalledTimes(1);
    input.dispatchEvent(new FocusEvent("blur"));
    expect(onCommit).toHaveBeenCalledTimes(2);
  });
});
