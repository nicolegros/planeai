import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import CodeMirrorDiffRefreshHarness from "./CodeMirrorDiffRefreshHarness.svelte";

describe("CodeMirrorDiff refresh", () => {
  let target: HTMLElement;
  let component: { refresh: (modified: string) => void } | undefined;
  let originalGetClientRects: typeof Range.prototype.getClientRects | undefined;

  beforeEach(() => {
    originalGetClientRects = Range.prototype.getClientRects;
    Object.defineProperty(Range.prototype, "getClientRects", {
      configurable: true,
      value: () => [] as unknown as DOMRectList,
    });
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(() => {
    if (component) unmount(component);
    if (originalGetClientRects) {
      Object.defineProperty(Range.prototype, "getClientRects", {
        configurable: true,
        value: originalGetClientRects,
      });
    } else {
      delete (Range.prototype as { getClientRects?: unknown }).getClientRects;
    }
    target.remove();
  });

  it("renders replacement content when the selected file diff refreshes", async () => {
    component = mount(CodeMirrorDiffRefreshHarness, { target });

    await vi.waitFor(() => expect(target.textContent).toContain("stale file content"));

    component.refresh("refreshed file content\n");

    await vi.waitFor(() => {
      expect(target.textContent).toContain("refreshed file content");
      expect(target.textContent).not.toContain("stale file content");
    });
  });
});
