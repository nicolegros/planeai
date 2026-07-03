import { describe, it, expect } from "vitest";
import { extractCommandName } from "../shell-title";

describe("extractCommandName", () => {
  it("extracts last path segment from a full path", () => {
    expect(extractCommandName("/usr/bin/vim")).toBe("vim");
    expect(extractCommandName("/Users/nicolas/.cargo/bin/cargo-nextest")).toBe("cargo-nextest");
  });

  it("returns binary name as-is for a simple command", () => {
    expect(extractCommandName("cargo")).toBe("cargo");
    expect(extractCommandName("node")).toBe("node");
    expect(extractCommandName("vim")).toBe("vim");
  });

  it("returns null for shell names (shell reset)", () => {
    expect(extractCommandName("zsh")).toBeNull();
    expect(extractCommandName("bash")).toBeNull();
    expect(extractCommandName("fish")).toBeNull();
    expect(extractCommandName("sh")).toBeNull();
    expect(extractCommandName("-zsh")).toBeNull();
    expect(extractCommandName("-bash")).toBeNull();
  });

  it("returns null for directory paths (cwd resets)", () => {
    expect(extractCommandName("~/projects/planeai")).toBeNull();
    expect(extractCommandName("~/src")).toBeNull();
    expect(extractCommandName("~")).toBeNull();
    expect(extractCommandName("/")).toBeNull();
    expect(extractCommandName("user@host:~/work")).toBeNull();
  });
});
