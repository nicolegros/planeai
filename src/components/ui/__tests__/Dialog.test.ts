import { describe, expect, it } from "vitest";
import dialogSource from "../Dialog.svelte?raw";

describe("Dialog focus styling", () => {
  it("suppresses the browser-default outline on the shared dialog content surface", () => {
    expect(dialogSource).toMatch(
      /<Dialog\.Content\s+class="[^"]*overflow-hidden[^"]*outline-none/,
    );
  });

  it("can suppress automatic dialog focus for an explicitly managed child target", () => {
    expect(dialogSource).toContain("preventOpenAutoFocus?: boolean;");
    expect(dialogSource).toContain("onOpenAutoFocus={(e) => { if (preventOpenAutoFocus) e.preventDefault(); }}");
  });
});
