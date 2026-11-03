/**
 * The clocks snoozes are read against. The lane clock moves only when a snooze runs out, so the
 * sidebar model rebuilds at wakes alone; the minute clock moves every minute, for countdown labels.
 * Timers stall while the computer sleeps, so the clock never waits more than a minute and re-reads
 * the time when the window comes back.
 */
import { MINUTE, soonestWake } from "./task-snooze";
import * as taskStore from "./task-store.svelte";

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
      // Counted from the clocks, not from now: task changes re-run this, and must neither postpone
      // the minute tick nor miss a snooze that arrives already run out (an undone wake).
      const wake = soonestWake(taskStore.getAllTasks(), laneNow);
      const next = wake === null ? minuteNow + MINUTE : Math.min(wake, minuteNow + MINUTE);
      const timer = setTimeout(sync, Math.max(0, next - Date.now()));
      return () => clearTimeout(timer);
    });
  });
  return () => {
    stopTimer();
    window.removeEventListener("focus", sync);
    document.removeEventListener("visibilitychange", syncWhenVisible);
  };
}
