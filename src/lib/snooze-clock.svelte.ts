/**
 * The clocks snoozes are read against. The lane clock moves only when a snooze runs out, so the
 * sidebar model rebuilds at wakes alone; the minute clock moves every minute, for countdown labels.
 * Timers stall while the computer sleeps, so the clock never waits more than a minute and re-reads
 * the time when the window comes back.
 */
import { soonestWake } from "./task-snooze";
import * as taskStore from "./task-store.svelte";

const MINUTE = 60_000;

let laneNow = $state(Date.now());
let minuteNow = $state(Date.now());

/** The time lanes are decided at: whether a task is snoozed or woke. */
export function getLaneNow(): number {
  return laneNow;
}

/** The time countdown labels are written at. */
export function getMinuteNow(): number {
  return minuteNow;
}

/** Moves both clocks to `real`; the lane clock only when a snooze ran out since it last moved. */
export function syncSnoozeClock(real = Date.now()): void {
  minuteNow = real;
  const wake = soonestWake(taskStore.getAllTasks(), laneNow);
  if (wake !== null && wake <= real) laneNow = real;
}

/** Runs the clock until the returned stop is called. */
export function startSnoozeClock(): () => void {
  const sync = () => syncSnoozeClock();
  const syncWhenVisible = () => {
    if (document.visibilityState === "visible") sync();
  };
  window.addEventListener("focus", sync);
  document.addEventListener("visibilitychange", syncWhenVisible);
  sync();
  const stopTimer = $effect.root(() => {
    $effect(() => {
      void minuteNow;
      const real = Date.now();
      const wake = soonestWake(taskStore.getAllTasks(), real);
      const delay = wake === null ? MINUTE : Math.min(MINUTE, Math.max(0, wake - real));
      const timer = setTimeout(sync, delay);
      return () => clearTimeout(timer);
    });
  });
  return () => {
    stopTimer();
    window.removeEventListener("focus", sync);
    document.removeEventListener("visibilitychange", syncWhenVisible);
  };
}
