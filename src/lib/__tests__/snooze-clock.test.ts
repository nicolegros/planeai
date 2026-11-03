import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { tasks } = vi.hoisted(() => ({ tasks: { current: [] as object[] } }));
vi.mock("../task-store.svelte", () => ({ getAllTasks: () => tasks.current }));

const at = (time: string) => Date.parse(`2026-11-03T${time}:00.000Z`);
const snoozedUntil = (time: string) => ({
  status: "in_progress",
  snoozed_until: new Date(at(time)).toISOString(),
  snoozed_at: new Date(at("08:00")).toISOString(),
  woke_at: null,
});

/**
 * The clock starts at the time of its import, so each test imports it under its own fake time, with
 * the Svelte runtime it was imported with.
 */
async function clockAt(time: string) {
  vi.setSystemTime(at(time));
  vi.resetModules();
  const { flushSync } = await import("svelte");
  return { ...(await import("../snooze-clock.svelte")), flushSync };
}

beforeEach(() => vi.useFakeTimers());

afterEach(() => {
  tasks.current = [];
  vi.useRealTimers();
});

describe("snooze clock", () => {
  it("moves the lane clock only when a snooze runs out, and the minute clock every time", async () => {
    const clock = await clockAt("09:00");
    tasks.current = [snoozedUntil("10:00")];

    clock.syncSnoozeClock(at("09:30"));
    const beforeWake = [clock.getLaneNow(), clock.getMinuteNow()];
    clock.syncSnoozeClock(at("10:00"));

    expect([beforeWake, [clock.getLaneNow(), clock.getMinuteNow()]]).toEqual([
      [at("09:00"), at("09:30")],
      [at("10:00"), at("10:00")],
    ]);
  });

  it("catches a wake that passed while the computer slept once the window comes back", async () => {
    const clock = await clockAt("09:00");
    tasks.current = [snoozedUntil("10:00")];
    const stop = clock.startSnoozeClock();

    vi.setSystemTime(at("11:15"));
    window.dispatchEvent(new Event("focus"));

    expect(clock.getLaneNow()).toBe(at("11:15"));
    stop();
  });

  it("never waits more than a minute between ticks", async () => {
    const clock = await clockAt("09:00");
    tasks.current = [snoozedUntil("12:00")];
    const stop = clock.startSnoozeClock();
    clock.flushSync();

    vi.advanceTimersByTime(60_000);

    expect([clock.getMinuteNow(), clock.getLaneNow()]).toEqual([at("09:01"), at("09:00")]);
    stop();
  });

  it("ticks right at a wake that comes sooner than a minute", async () => {
    const clock = await clockAt("09:59");
    tasks.current = [
      { ...snoozedUntil("10:00"), snoozed_until: new Date(at("09:59") + 20_000).toISOString() },
    ];
    const stop = clock.startSnoozeClock();
    clock.flushSync();

    vi.advanceTimersByTime(20_000);

    expect(clock.getLaneNow()).toBe(at("09:59") + 20_000);
    stop();
  });
});
