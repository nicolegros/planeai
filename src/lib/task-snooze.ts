/**
 * Snooze: a task set aside until a time, then woken. The user's snooze writes `snoozed_until` and
 * `snoozed_at`; the backend writes `woke_at` when an agent asks for attention or exits meanwhile;
 * a manual wake or Done clears all three. A natural wake writes nothing: it is derived from the clock.
 */
import type { StoredSnooze, TaskItem, TaskSnooze } from "./types";

type SnoozeFields = Pick<TaskItem, "status" | "snoozed_until" | "woke_at">;
type WokeFields = SnoozeFields & Pick<TaskItem, "snoozed_at" | "last_visited_at">;

export const DONE_CANNOT_SNOOZE = "Done tasks cannot be snoozed.";
export const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** A snooze still running at `now`, whatever the task's status. */
export function isSnoozeActive(
  snooze: Pick<TaskSnooze, "snoozed_until" | "woke_at">,
  now: number,
): boolean {
  return (
    snooze.woke_at === null &&
    snooze.snoozed_until !== null &&
    now < Date.parse(snooze.snoozed_until)
  );
}

export function isTaskSnoozed(task: SnoozeFields, now: number): boolean {
  return task.status !== "done" && isSnoozeActive(task, now);
}

export function snoozeOf({ snoozed_until, snoozed_at, woke_at }: TaskSnooze): TaskSnooze {
  return { snoozed_until, snoozed_at, woke_at };
}

/**
 * `task` showing `snooze` if it was written after the snooze `task` shows. The replies to the user's
 * writes, an early wake's event and reloads arrive in any order; the revision says which is newest.
 */
export function withNewerSnooze<T extends StoredSnooze>(task: T, snooze: StoredSnooze): T {
  if (snooze.snooze_rev <= task.snooze_rev) return task;
  return { ...task, ...snoozeOf(snooze), snooze_rev: snooze.snooze_rev };
}

/** When a snoozed task woke, early or at its time; null while it sleeps or was never snoozed. */
export function taskWokeAt(task: WokeFields, now: number): number | null {
  if (task.snoozed_at === null) return null;
  if (task.woke_at !== null) return Date.parse(task.woke_at);
  const until = task.snoozed_until === null ? NaN : Date.parse(task.snoozed_until);
  return until <= now ? until : null;
}

/** Woke since the user last looked at the task. */
export function isTaskWoke(task: WokeFields, now: number): boolean {
  const woke = taskWokeAt(task, now);
  if (task.status === "done" || woke === null) return false;
  return task.last_visited_at === null || woke > Date.parse(task.last_visited_at);
}

/** The soonest time a snoozed task wakes, among `tasks`. */
export function soonestWake(tasks: Iterable<SnoozeFields>, now: number): number | null {
  let soonest: number | null = null;
  for (const task of tasks) {
    if (!isTaskSnoozed(task, now)) continue;
    const until = Date.parse(task.snoozed_until!);
    if (soonest === null || until < soonest) soonest = until;
  }
  return soonest;
}

/** Time left until `until`, in its largest whole unit: "45m", "3h", "2d", or "<1m". */
export function snoozeCountdown(until: number, now: number): string {
  const left = until - now;
  if (left < MINUTE) return "<1m";
  if (left < HOUR) return `${Math.floor(left / MINUTE)}m`;
  if (left < DAY) return `${Math.floor(left / HOUR)}h`;
  return `${Math.floor(left / DAY)}d`;
}

/** "Tue 09:00", or "Tue 9:00 AM" where the locale uses a 12-hour clock. */
export function wakeTimeLabel(at: number, locale?: string): string {
  const weekday = new Intl.DateTimeFormat(locale, { weekday: "short" }).format(at);
  return `${weekday} ${timeLabel(at, locale)}`;
}

/** The locale's short time, which follows its 12 or 24-hour clock. */
function timeLabel(at: number, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { timeStyle: "short" }).format(at);
}

/**
 * Picker presets, soonest first. "after" counts from the click; "at" is a local wall-clock time on
 * a day counted from today (`weekday` picks the next such weekday strictly after today).
 */
type SnoozePreset =
  | { label: string; kind: "after"; minutes: number }
  | {
      label: string;
      kind: "at";
      day: { daysAhead: number } | { weekday: number };
      hour: number;
      minute: number;
    };

const SNOOZE_PRESETS: SnoozePreset[] = [
  { label: "In 1 hour", kind: "after", minutes: 60 },
  { label: "In 3 hours", kind: "after", minutes: 180 },
  { label: "This evening", kind: "at", day: { daysAhead: 0 }, hour: 18, minute: 0 },
  { label: "Tomorrow", kind: "at", day: { daysAhead: 1 }, hour: 9, minute: 0 },
  { label: "Next week", kind: "at", day: { weekday: 1 }, hour: 9, minute: 0 },
];

function presetTime(preset: SnoozePreset, now: Date): Date {
  if (preset.kind === "after") return new Date(now.getTime() + preset.minutes * MINUTE);
  const daysAhead =
    "daysAhead" in preset.day
      ? preset.day.daysAhead
      : ((preset.day.weekday - now.getDay() + 6) % 7) + 1;
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + daysAhead,
    preset.hour,
    preset.minute,
  );
}

function presetLabel(preset: SnoozePreset, at: Date, locale?: string): string {
  if (preset.kind === "after") return preset.label;
  const time =
    "weekday" in preset.day ? wakeTimeLabel(at.getTime(), locale) : timeLabel(at.getTime(), locale);
  return `${preset.label} (${time})`;
}

export interface SnoozeOption {
  label: string;
  /** The time to snooze to when clicked: "In N hours" counts from the click, a clock time is the one shown. */
  resolve: (now: Date) => Date;
  /** The time as of when the options were listed. */
  at: Date;
}

/** The presets still ahead of `now`, soonest first, one per minute. */
export function snoozeOptions(now: Date, locale?: string): SnoozeOption[] {
  const options = SNOOZE_PRESETS.map((preset) => {
    const at = presetTime(preset, now);
    return {
      label: presetLabel(preset, at, locale),
      resolve: (clicked: Date) => (preset.kind === "after" ? presetTime(preset, clicked) : at),
      at,
    };
  })
    .filter((option) => option.at.getTime() > now.getTime())
    .sort((a, b) => a.at.getTime() - b.at.getTime());
  const minutes = new Set<number>();
  return options.filter((option) => {
    const minute = Math.floor(option.at.getTime() / MINUTE);
    if (minutes.has(minute)) return false;
    minutes.add(minute);
    return true;
  });
}

export type CustomSnooze =
  | { mode: "at"; date: string; time: string }
  | { mode: "after"; amount: string; unit: "minutes" | "hours" | "days" };

const UNIT_MS = { minutes: MINUTE, hours: HOUR, days: DAY };

/** A custom snooze time from the dialog's fields: a local date and time, or an amount from now. */
export function customSnoozeTime(input: CustomSnooze, now: Date): { at: Date } | { error: string } {
  let at: Date;
  if (input.mode === "after") {
    if (!/^\d+$/.test(input.amount.trim()) || Number(input.amount) === 0)
      return { error: "Enter a whole number above 0." };
    at = new Date(now.getTime() + Number(input.amount) * UNIT_MS[input.unit]);
  } else {
    const date = /^(\d{4})-(\d{2})-(\d{2})$/.exec(input.date);
    const time = /^(\d{2}):(\d{2})$/.exec(input.time);
    if (!date || !time) return { error: "Pick a date and a time." };
    const [year, month, day, hour, minute] = [...date.slice(1), ...time.slice(1)].map(Number);
    at = new Date(year, month - 1, day, hour, minute);
    // An invalid day rolls over into the next month, and a time in a clock-change gap shifts.
    const exists =
      at.getFullYear() === year &&
      at.getMonth() === month - 1 &&
      at.getDate() === day &&
      at.getHours() === hour &&
      at.getMinutes() === minute;
    if (!exists) return { error: "That time does not exist." };
  }
  // The backend stores times as RFC 3339 text, which has four-digit years.
  if (Number.isNaN(at.getTime()) || at.getUTCFullYear() > 9999)
    return { error: "That is too far ahead." };
  if (at.getTime() <= now.getTime()) return { error: "That time has passed." };
  return { at };
}
