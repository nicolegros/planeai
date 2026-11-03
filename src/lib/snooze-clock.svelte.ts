/**
 * The one clock snoozes are read against. It ticks only when the sidebar would change: at the soonest
 * wake, and once a minute while a countdown is on screen.
 */
import { nextClockTick, soonestWake } from "./task-snooze";
import * as taskStore from "./task-store.svelte";

// setTimeout fires at once past this delay.
const MAX_TIMEOUT_MS = 2_147_483_647;

let now = $state(Date.now());

export function getNow(): number {
  return now;
}

/** Keeps the clock ticking while it runs; `countdownShown` tells whether a countdown is visible. */
export function startSnoozeClock(countdownShown: () => boolean): () => void {
  return $effect.root(() => {
    $effect(() => {
      void now;
      const real = Date.now();
      const next = nextClockTick(
        real,
        soonestWake(taskStore.getAllTasks(), real),
        countdownShown(),
      );
      if (next === null) return;
      const timer = setTimeout(
        () => (now = Date.now()),
        Math.min(Math.max(0, next - real), MAX_TIMEOUT_MS),
      );
      return () => clearTimeout(timer);
    });
  });
}
