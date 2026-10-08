import { describe, expect, it } from "vitest";
import appSource from "../../App.svelte?raw";

describe("Cmd+N new-item chooser", () => {
  it("does not steal terminal focus while the chooser mounts", () => {
    expect(appSource).toMatch(
      /<SharedDialog open=\{true\}[\s\S]*?title="New…"[\s\S]*?preventOpenAutoFocus=\{true\}/,
    );
  });

  it("offers a Session choice with the S mnemonic", () => {
    expect(appSource).toMatch(/>Session<\/span>[\s\S]*?>s<\/span>/);
  });
});
