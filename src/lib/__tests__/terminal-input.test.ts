import { describe, expect, it, vi } from "vitest";
import { writeUserInput } from "../terminal-input";

describe("writeUserInput", () => {
  it("invalidates dependent state before writing bytes to the PTY", () => {
    const events: string[] = [];
    const invalidate = vi.fn(() => events.push("invalidate"));
    const write = vi.fn(() => events.push("write"));

    writeUserInput([0x0d], invalidate, write);

    expect(events).toEqual(["invalidate", "write"]);
    expect(write).toHaveBeenCalledWith([0x0d]);
  });
});
