import { describe, expect, it } from "vitest";
import reviewTabSource from "../ReviewTab.svelte?raw";

describe("ReviewTab nested diff sidebar keyboard contract", () => {
  it("renders folder and file treeitems as roving-tabindex focus targets", () => {
    expect(reviewTabSource).toContain('role="treeitem"');
    expect(reviewTabSource).toContain("tabindex={sidebarFocusedRowId === rowId ? 0 : -1}");
    expect(reviewTabSource).toContain("onfocus={() => (sidebarFocusedRowId = rowId)}");
    expect(reviewTabSource).toContain("onkeydown={handleSidebarRowKeydown}");
    expect(reviewTabSource).toContain("?.focus({ preventScroll: true });");
  });

  it("lets platform-modified arrows reach the existing global file navigation", () => {
    expect(reviewTabSource).toContain("const hasPlatformModifier = e.ctrlKey || e.metaKey;");
    expect(reviewTabSource).toContain(
      'const isDown = !hasPlatformModifier && (e.key === "ArrowDown" || e.key === "j");',
    );
    expect(reviewTabSource).toContain(
      'const isUp = !hasPlatformModifier && (e.key === "ArrowUp" || e.key === "k");',
    );
    expect(reviewTabSource).toContain(
      'if ((e.key === "n" && e.ctrlKey) || (e.key === "ArrowDown" && e.ctrlKey))',
    );
    expect(reviewTabSource).toContain(
      'if ((e.key === "p" && e.ctrlKey) || (e.key === "ArrowUp" && e.ctrlKey))',
    );
  });
});
