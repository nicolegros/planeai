import { describe, expect, it } from "vitest";
import {
  customSnoozeTime,
  soonestWake,
  isTaskSnoozed,
  isTaskWoke,
  snoozeCountdown,
  snoozeOptions,
  taskWokeAt,
  wakeTimeLabel,
} from "../task-snooze";

const UNTIL = "2026-11-04T09:00:00.000+00:00";
const until = Date.parse(UNTIL);

function snoozed(fields: Partial<Parameters<typeof isTaskWoke>[0]> = {}) {
  return {
    status: "in_progress",
    snoozed_until: UNTIL,
    snoozed_at: "2026-11-03T10:00:00.000+00:00",
    woke_at: null,
    last_visited_at: "2026-11-03T10:00:00.000+00:00",
    ...fields,
  };
}

describe("isTaskSnoozed", () => {
  it("is snoozed until the instant it wakes", () => {
    expect([
      isTaskSnoozed(snoozed(), until - 1),
      isTaskSnoozed(snoozed(), until),
      isTaskSnoozed(snoozed(), until + 1),
    ]).toEqual([true, false, false]);
  });

  it("is not snoozed once woken early, never snoozed, or done", () => {
    expect([
      isTaskSnoozed(snoozed({ woke_at: "2026-11-03T12:00:00.000+00:00" }), until - 1),
      isTaskSnoozed(snoozed({ snoozed_until: null, snoozed_at: null }), until - 1),
      isTaskSnoozed(snoozed({ status: "done" }), until - 1),
    ]).toEqual([false, false, false]);
  });
});

describe("taskWokeAt and isTaskWoke", () => {
  it("wakes at the snooze time, and shows Woke until the task is visited after it", () => {
    expect(taskWokeAt(snoozed(), until - 1)).toBe(null);
    expect(taskWokeAt(snoozed(), until)).toBe(until);
    expect([
      isTaskWoke(snoozed(), until - 1),
      isTaskWoke(snoozed(), until),
      isTaskWoke(snoozed({ last_visited_at: UNTIL }), until),
      isTaskWoke(snoozed({ last_visited_at: "2026-11-04T09:00:00.001Z" }), until),
      isTaskWoke(snoozed({ last_visited_at: null }), until),
    ]).toEqual([false, true, false, false, true]);
  });

  it("wakes early at woke_at", () => {
    const early = snoozed({ woke_at: "2026-11-03T12:00:00.000+00:00" });
    expect(taskWokeAt(early, until - 1)).toBe(Date.parse("2026-11-03T12:00:00Z"));
    expect(isTaskWoke(early, until - 1)).toBe(true);
  });

  it("never shows Woke for a task never snoozed, or done", () => {
    const never = snoozed({ snoozed_until: null, snoozed_at: null });
    expect([isTaskWoke(never, until), isTaskWoke(snoozed({ status: "done" }), until)]).toEqual([
      false,
      false,
    ]);
  });
});

describe("snoozeCountdown", () => {
  const MINUTE = 60_000;
  it("counts down in the largest whole unit", () => {
    const labels = [
      30_000,
      MINUTE,
      45 * MINUTE,
      60 * MINUTE,
      179 * MINUTE,
      180 * MINUTE,
      23 * 60 * MINUTE + 30_000,
      24 * 60 * MINUTE,
      6 * 24 * 60 * MINUTE,
    ].map((left) => snoozeCountdown(until, until - left));
    expect(labels).toEqual(["<1m", "1m", "45m", "1h", "2h", "3h", "23h", "1d", "6d"]);
  });
});

describe("wakeTimeLabel", () => {
  it("names the weekday and time in the given locale", () => {
    const at = new Date(2026, 10, 10, 9, 0).getTime();
    expect(wakeTimeLabel(at, "en-GB")).toBe("Tue 09:00");
    expect(wakeTimeLabel(at, "en-US")).toBe("Tue 9:00 AM");
  });
});

describe("snoozeOptions", () => {
  const labelsAt = (now: Date) =>
    snoozeOptions(now, "en-GB").map((option) => [option.label, option.at.toString()]);
  const local = (day: number, hour: number, minute = 0) =>
    new Date(2026, 10, day, hour, minute).toString();

  it("offers every preset, soonest first, before the evening", () => {
    // Tuesday 3 November 2026, 10:15.
    expect(labelsAt(new Date(2026, 10, 3, 10, 15))).toEqual([
      ["In 1 hour", local(3, 11, 15)],
      ["In 3 hours", local(3, 13, 15)],
      ["This evening (18:00)", local(3, 18)],
      ["Tomorrow (09:00)", local(4, 9)],
      ["Next week (Mon 09:00)", local(9, 9)],
    ]);
  });

  it("drops a time that passed today and one that falls on the same minute as another", () => {
    expect(labelsAt(new Date(2026, 10, 3, 17, 0)).map(([label]) => label)).toEqual([
      "In 1 hour",
      "In 3 hours",
      "Tomorrow (09:00)",
      "Next week (Mon 09:00)",
    ]);
    expect(labelsAt(new Date(2026, 10, 3, 19, 30)).map(([label]) => label)).toEqual([
      "In 1 hour",
      "In 3 hours",
      "Tomorrow (09:00)",
      "Next week (Mon 09:00)",
    ]);
  });

  it("on Sunday, next week is tomorrow and shows once", () => {
    expect(labelsAt(new Date(2026, 10, 8, 10, 0))).toEqual([
      ["In 1 hour", local(8, 11)],
      ["In 3 hours", local(8, 13)],
      ["This evening (18:00)", local(8, 18)],
      ["Tomorrow (09:00)", local(9, 9)],
    ]);
  });

  it("on Monday morning, next week is a week away", () => {
    expect(labelsAt(new Date(2026, 10, 9, 8, 0))).toEqual([
      ["In 1 hour", local(9, 9)],
      ["In 3 hours", local(9, 11)],
      ["This evening (18:00)", local(9, 18)],
      ["Tomorrow (09:00)", local(10, 9)],
      ["Next week (Mon 09:00)", local(16, 9)],
    ]);
  });
});

describe("customSnoozeTime", () => {
  const now = new Date(2026, 10, 3, 10, 0);

  it("takes a local date and time, or an amount from now", () => {
    expect(customSnoozeTime({ mode: "at", date: "2026-11-04", time: "09:30" }, now)).toEqual({
      at: new Date(2026, 10, 4, 9, 30),
    });
    expect(customSnoozeTime({ mode: "after", amount: "2", unit: "days" }, now)).toEqual({
      at: new Date(now.getTime() + 2 * 24 * 3_600_000),
    });
    expect(customSnoozeTime({ mode: "after", amount: "90", unit: "minutes" }, now)).toEqual({
      at: new Date(2026, 10, 3, 11, 30),
    });
  });

  it("rejects past, empty and impossible times", () => {
    expect([
      customSnoozeTime({ mode: "at", date: "2026-11-03", time: "10:00" }, now),
      customSnoozeTime({ mode: "at", date: "", time: "09:00" }, now),
      customSnoozeTime({ mode: "at", date: "2026-02-30", time: "09:00" }, now),
      customSnoozeTime({ mode: "after", amount: "0", unit: "hours" }, now),
      customSnoozeTime({ mode: "after", amount: "1.5", unit: "hours" }, now),
    ]).toEqual([
      { error: "That time has passed." },
      { error: "Pick a date and a time." },
      { error: "That time does not exist." },
      { error: "Enter a whole number above 0." },
      { error: "Enter a whole number above 0." },
    ]);
  });
});

describe("soonestWake", () => {
  it("is the earliest wake among tasks still asleep", () => {
    const tasks = [
      snoozed({ snoozed_until: "2026-11-05T09:00:00.000+00:00" }),
      snoozed(),
      snoozed({ snoozed_until: "2026-11-03T11:00:00.000+00:00", woke_at: "2026-11-03T10:30:00Z" }),
      snoozed({ snoozed_until: "2026-11-03T12:00:00.000+00:00", status: "done" }),
    ];
    expect(soonestWake(tasks, Date.parse("2026-11-03T10:00:00Z"))).toBe(until);
    expect(soonestWake(tasks, until + 1)).toBe(Date.parse("2026-11-05T09:00:00Z"));
    expect(soonestWake([], until)).toBe(null);
  });
});
