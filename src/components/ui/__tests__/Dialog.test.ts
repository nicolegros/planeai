import { describe, expect, it } from "vitest";
import dialogSource from "../Dialog.svelte?raw";

describe("Dialog focus styling", () => {
  it("suppresses the browser-default outline on the shared dialog content surface", () => {
    expect(dialogSource).toMatch(/<Dialog\.Content\s+class="[^"]*overflow-hidden[^"]*outline-none/);
  });

  it("exposes a prop to suppress automatic dialog focus for a managed child target", () => {
    // Behaviour is covered by src/components/__tests__/dialog-form-focus.test.ts,
    // which mounts the real Dialog; only the prop contract is pinned here.
    expect(dialogSource).toContain("preventOpenAutoFocus?: boolean;");
  });
});
