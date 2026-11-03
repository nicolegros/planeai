import { describe, expect, it } from "vitest";
import { isTaskUnread } from "../task-unread";

describe("isTaskUnread", () => {
  it("is unread only when activity came after the last visit, or was never visited", () => {
    const cases = [
      { last_activity_at: null, last_visited_at: null },
      { last_activity_at: null, last_visited_at: "2026-11-03T10:00:00.000+00:00" },
      { last_activity_at: "2026-11-03T10:00:00.000+00:00", last_visited_at: null },
      {
        last_activity_at: "2026-11-03T10:00:00.500Z",
        last_visited_at: "2026-11-03T10:00:00.499Z",
      },
      {
        last_activity_at: "2026-11-03T10:00:00.500Z",
        last_visited_at: "2026-11-03T10:00:00.500+00:00",
      },
      {
        last_activity_at: "2026-11-03T10:00:00Z",
        last_visited_at: "2026-11-03T11:00:00+01:00",
      },
      {
        last_activity_at: "2026-11-03T10:00:01Z",
        last_visited_at: "2026-11-03T10:00:00.999Z",
      },
    ];
    expect(cases.map(isTaskUnread)).toEqual([false, false, true, true, false, false, true]);
  });
});
