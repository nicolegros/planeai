import { describe, expect, it } from "vitest";
import {
  agentPtyKey,
  diffPtyKey,
  editorPtyKey,
  parsePtyKey,
  ptyKeySessionId,
  shellIndex,
  shellPtyKey,
} from "../pty-key";

describe("pty key grammar", () => {
  it("round-trips every resource kind", () => {
    expect(parsePtyKey(agentPtyKey("s1"))).toEqual({ kind: "agent", sessionId: "s1" });
    expect(parsePtyKey(shellPtyKey("s1", 3))).toEqual({ kind: "shell", sessionId: "s1", index: 3 });
    expect(parsePtyKey(diffPtyKey("s1"))).toEqual({ kind: "diff", sessionId: "s1" });
    expect(parsePtyKey(editorPtyKey("s1", "src/a.ts"))).toEqual({
      kind: "editor",
      sessionId: "s1",
      filePath: "src/a.ts",
    });
  });

  it("keeps colons inside an editor file path", () => {
    expect(parsePtyKey("s1:editor:C:/repo/a:b.ts")).toEqual({
      kind: "editor",
      sessionId: "s1",
      filePath: "C:/repo/a:b.ts",
    });
  });

  it("rejects keys outside the grammar", () => {
    for (const key of ["", ":1", "s1:0", "s1:-1", "s1:01", "s1:x", "s1:editor:"]) {
      expect(parsePtyKey(key), key).toBeNull();
    }
  });

  it("finds the owning session of any resource", () => {
    expect(ptyKeySessionId("s1")).toBe("s1");
    expect(ptyKeySessionId("s1:2")).toBe("s1");
    expect(ptyKeySessionId("s1:diff")).toBe("s1");
    expect(ptyKeySessionId("s1:editor:a/b:c")).toBe("s1");
  });

  it("reads a shell index only from shell keys", () => {
    expect(shellIndex("s1:4")).toBe(4);
    expect(shellIndex("s1")).toBeNull();
    expect(shellIndex("s1:diff")).toBeNull();
  });
});
