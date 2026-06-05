import { describe, it, expect, vi, beforeEach } from "vitest";
import { touchMru, removeMru, getMruList } from "../mru.svelte";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

import { invoke } from "@tauri-apps/api/core";

describe("MRU persistence", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    for (const id of getMruList()) removeMru(id);
    vi.mocked(invoke).mockClear();
  });

  it("persists on every touchMru call", () => {
    touchMru("a");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("save_mru_order", {
      sessionIds: ["a"],
    });

    touchMru("b");
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("save_mru_order", {
      sessionIds: ["b", "a"],
    });
  });

  it("sends correct order after reordering", () => {
    touchMru("a");
    touchMru("b");
    touchMru("c");
    touchMru("a");

    expect(invoke).toHaveBeenLastCalledWith("save_mru_order", {
      sessionIds: ["a", "c", "b"],
    });
  });
});
