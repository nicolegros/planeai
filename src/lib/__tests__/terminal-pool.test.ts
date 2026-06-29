import { describe, it, expect, beforeEach } from "vitest";
import { getMruList, activateSession, removeSession, isMounted } from "../mru.svelte";

describe("mru pool", () => {
  beforeEach(() => {
    for (const id of getMruList()) removeSession(id);
  });

  it("activates and mounts a session", () => {
    activateSession("s1");
    expect(getMruList()).toContain("s1");
    expect(isMounted("s1")).toBe(true);
  });

  it("keeps all sessions mounted regardless of count", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    activateSession("s4");
    expect(isMounted("s1")).toBe(true);
    expect(isMounted("s2")).toBe(true);
    expect(isMounted("s3")).toBe(true);
    expect(isMounted("s4")).toBe(true);
  });

  it("promotes activated session to front of MRU", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s1");
    expect(getMruList()[0]).toBe("s1");
  });

  it("removeSession evicts from MRU", () => {
    activateSession("s1");
    activateSession("s2");
    removeSession("s1");
    expect(isMounted("s1")).toBe(false);
    expect(isMounted("s2")).toBe(true);
  });

  it("isMounted returns false for unknown sessions", () => {
    expect(isMounted("unknown")).toBe(false);
  });
});
