import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { touchMru, removeMru, getMruList } from "../mru.svelte";

vi.mock("../api", () => ({
  sessions: {
    saveMruOrder: vi.fn(() => Promise.resolve()),
  },
}));

import { sessions } from "../api";

describe("MRU persistence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(sessions.saveMruOrder).mockClear();
    for (const id of getMruList()) removeMru(id);
    vi.runAllTimers();
    vi.mocked(sessions.saveMruOrder).mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("debounces persistence across rapid touchMru calls", () => {
    touchMru("a");
    touchMru("b");
    expect(sessions.saveMruOrder).not.toHaveBeenCalled();

    vi.runAllTimers();
    expect(sessions.saveMruOrder).toHaveBeenCalledTimes(1);
    expect(sessions.saveMruOrder).toHaveBeenCalledWith(["b", "a"]);
  });

  it("sends correct order after reordering", () => {
    touchMru("a");
    touchMru("b");
    touchMru("c");
    touchMru("a");

    vi.runAllTimers();
    expect(sessions.saveMruOrder).toHaveBeenCalledTimes(1);
    expect(sessions.saveMruOrder).toHaveBeenLastCalledWith(["a", "c", "b"]);
  });
});
