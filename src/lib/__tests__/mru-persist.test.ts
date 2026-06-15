import { describe, it, expect, vi, beforeEach } from "vitest";
import { touchMru, removeMru, getMruList } from "../mru.svelte";

vi.mock("../api", () => ({
  sessions: {
    saveMruOrder: vi.fn(() => Promise.resolve()),
  },
}));

import { sessions } from "../api";

describe("MRU persistence", () => {
  beforeEach(() => {
    vi.mocked(sessions.saveMruOrder).mockClear();
    for (const id of getMruList()) removeMru(id);
    vi.mocked(sessions.saveMruOrder).mockClear();
  });

  it("persists on every touchMru call", () => {
    touchMru("a");
    expect(sessions.saveMruOrder).toHaveBeenCalledTimes(1);
    expect(sessions.saveMruOrder).toHaveBeenCalledWith(["a"]);

    touchMru("b");
    expect(sessions.saveMruOrder).toHaveBeenCalledTimes(2);
    expect(sessions.saveMruOrder).toHaveBeenLastCalledWith(["b", "a"]);
  });

  it("sends correct order after reordering", () => {
    touchMru("a");
    touchMru("b");
    touchMru("c");
    touchMru("a");

    expect(sessions.saveMruOrder).toHaveBeenLastCalledWith(["a", "c", "b"]);
  });
});
