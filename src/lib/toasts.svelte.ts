/**
 * App-wide toast stack. Toasts are data: the host renders them and maps keys to
 * `actions`, so every toast is reachable from the keyboard (Mod+U) the same way.
 */

export type ToastKind = "error" | "warning" | "info" | "success";

export interface ToastAction {
  /** A single lowercase letter or "Enter"; see RESERVED_TOAST_KEYS. */
  key: string;
  label: string;
  run: () => void | Promise<void>;
}

/** "default" removes the toast, "disabled" refuses, a function runs after removal. */
export type ToastDismiss = "default" | "disabled" | (() => void | Promise<void>);

export interface ToastCountdown {
  seconds: number;
  label: (remaining: number) => string;
  onExpire: () => void | Promise<void>;
}

export interface ToastOptions {
  /** Showing a toast with an existing id replaces it in place. */
  id?: string;
  kind?: ToastKind;
  message: string;
  detail?: string;
  actions?: ToastAction[];
  dismiss?: ToastDismiss;
  /** Shown instead of "dismiss" on the dismiss button and in the key legend. */
  dismissLabel?: string;
  countdown?: ToastCountdown;
  autoDismissMs?: number;
  hiddenDuringOnboarding?: boolean;
}

export interface Toast {
  id: string;
  kind: ToastKind;
  message: string;
  detail?: string;
  actions: ToastAction[];
  dismiss: ToastDismiss;
  dismissLabel: string;
  countdown?: ToastCountdown;
  countdownLabel?: string;
  autoDismissMs?: number;
  hiddenDuringOnboarding: boolean;
  count: number;
}

export type ToastHoldReason = "focus" | "hover";

/** Keys the host owns on every toast: dismiss, copy, leave, and move between toasts. */
export const RESERVED_TOAST_KEYS = ["d", "c", "Escape", "ArrowUp", "ArrowDown"] as const;
export const TOAST_LIMIT = 4;
const SUCCESS_AUTO_DISMISS_MS = 3_000;

/** A pausable one-shot deadline that may tick at whole-second boundaries. */
interface Clock {
  remainingMs: number;
  startedAt: number | null;
  handle: ReturnType<typeof setTimeout> | null;
  tickEverySecond: boolean;
  onTick: (remainingMs: number) => void;
  onExpire: () => void;
}

interface Runtime {
  holds: Set<ToastHoldReason>;
  clocks: Clock[];
}

let toasts = $state<Toast[]>([]);
const runtimes = new Map<string, Runtime>();
let nextId = 0;

export function getToasts(): Toast[] {
  return toasts;
}

export function showToast(message: string, kind?: ToastKind): string;
export function showToast(options: ToastOptions): string;
export function showToast(input: string | ToastOptions, kind: ToastKind = "error"): string {
  const options: ToastOptions = typeof input === "string" ? { message: input, kind } : input;
  const actions = options.actions ?? [];
  assertValidActions(actions);

  const toast: Toast = {
    id: options.id ?? `toast-${++nextId}`,
    kind: options.kind ?? "error",
    message: options.message,
    detail: options.detail,
    actions,
    dismiss: options.dismiss ?? "default",
    dismissLabel: options.dismissLabel ?? "Dismiss",
    countdown: options.countdown,
    countdownLabel: options.countdown?.label(options.countdown.seconds),
    autoDismissMs:
      options.autoDismissMs ??
      (isPlain(options) && options.kind === "success" ? SUCCESS_AUTO_DISMISS_MS : undefined),
    hiddenDuringOnboarding: options.hiddenDuringOnboarding ?? false,
    count: 1,
  };

  const existing = options.id ? toasts.findIndex((candidate) => candidate.id === options.id) : -1;
  if (existing >= 0) {
    stopClocks(toast.id);
    toasts[existing] = toast;
    startClocks(toast);
    return toast.id;
  }

  if (isPlain(toast)) {
    const duplicate = toasts.find(
      (candidate) =>
        isPlain(candidate) && candidate.kind === toast.kind && candidate.message === toast.message,
    );
    if (duplicate) {
      duplicate.count += 1;
      stopClocks(duplicate.id);
      startClocks(duplicate);
      return duplicate.id;
    }
  }

  toasts.push(toast);
  startClocks(toast);
  enforceLimit(toast.id);
  return toast.id;
}

/** Remove a toast without running any of its handlers. */
export function dismissToast(id: string): void {
  stopClocks(id);
  runtimes.delete(id);
  toasts = toasts.filter((toast) => toast.id !== id);
}

/** User-initiated dismiss. Returns false when the toast refuses to be dismissed. */
export function requestDismissToast(id: string): boolean {
  const toast = toasts.find((candidate) => candidate.id === id);
  if (!toast || toast.dismiss === "disabled") return false;
  const { dismiss } = toast;
  dismissToast(id);
  if (typeof dismiss === "function") invoke(dismiss);
  return true;
}

/**
 * Remove the toast, then run the action bound to `key`. Removing first lets an
 * action replace its own toast (the update toast turns into "Installing...").
 */
export function runToastAction(id: string, key: string): boolean {
  const action = toasts
    .find((candidate) => candidate.id === id)
    ?.actions.find((candidate) => candidate.key === key);
  if (!action) return false;
  dismissToast(id);
  invoke(action.run);
  return true;
}

/** Pause a toast's timers until every hold is released. */
export function holdToast(id: string, reason: ToastHoldReason): void {
  const runtime = runtimes.get(id);
  if (!runtime) return;
  const wasHeld = runtime.holds.size > 0;
  runtime.holds.add(reason);
  if (!wasHeld) runtime.clocks.forEach(pauseClock);
}

export function releaseToast(id: string, reason: ToastHoldReason): void {
  const runtime = runtimes.get(id);
  if (!runtime || !runtime.holds.delete(reason)) return;
  if (runtime.holds.size === 0) runtime.clocks.forEach(runClock);
}

export function _resetForTests(): void {
  for (const id of runtimes.keys()) stopClocks(id);
  runtimes.clear();
  toasts = [];
  nextId = 0;
}

function isPlain(toast: Pick<ToastOptions, "actions" | "countdown" | "dismiss">): boolean {
  return !toast.actions?.length && !toast.countdown && (toast.dismiss ?? "default") === "default";
}

function assertValidActions(actions: ToastAction[]): void {
  const seen = new Set<string>();
  for (const { key, label } of actions) {
    if (
      (RESERVED_TOAST_KEYS as readonly string[]).includes(key) ||
      !(key === "Enter" || /^[a-z]$/.test(key))
    ) {
      throw new Error(`Toast action "${label}" cannot use key "${key}"`);
    }
    if (seen.has(key)) throw new Error(`Toast actions share the key "${key}"`);
    seen.add(key);
  }
}

function enforceLimit(newestId: string): void {
  while (toasts.length > TOAST_LIMIT) {
    const evictable = toasts.filter((toast) => toast.id !== newestId && isPlain(toast));
    const victim = evictable.find((toast) => toast.autoDismissMs !== undefined) ?? evictable[0];
    if (!victim) return;
    dismissToast(victim.id);
  }
}

function invoke(handler: () => void | Promise<void>): void {
  try {
    void Promise.resolve(handler()).catch(reportFailure);
  } catch (error) {
    reportFailure(error);
  }
}

function reportFailure(error: unknown): void {
  showToast(String(error), "error");
}

function startClocks(toast: Toast): void {
  const clocks: Clock[] = [];
  const { id, countdown, autoDismissMs } = toast;
  if (autoDismissMs !== undefined) {
    clocks.push(
      createClock(
        autoDismissMs,
        false,
        () => {},
        () => dismissToast(id),
      ),
    );
  }
  if (countdown) {
    clocks.push(
      createClock(
        countdown.seconds * 1_000,
        true,
        (remainingMs) => {
          const current = toasts.find((candidate) => candidate.id === id);
          if (current) current.countdownLabel = countdown.label(Math.ceil(remainingMs / 1_000));
        },
        () => {
          dismissToast(id);
          invoke(countdown.onExpire);
        },
      ),
    );
  }
  const runtime: Runtime = { holds: runtimes.get(id)?.holds ?? new Set(), clocks };
  runtimes.set(id, runtime);
  if (runtime.holds.size === 0) clocks.forEach(runClock);
}

function stopClocks(id: string): void {
  runtimes.get(id)?.clocks.forEach(pauseClock);
}

function createClock(
  durationMs: number,
  tickEverySecond: boolean,
  onTick: (remainingMs: number) => void,
  onExpire: () => void,
): Clock {
  return {
    remainingMs: durationMs,
    startedAt: null,
    handle: null,
    tickEverySecond,
    onTick,
    onExpire,
  };
}

function runClock(clock: Clock): void {
  if (clock.handle) return;
  const delay = clock.tickEverySecond ? clock.remainingMs % 1_000 || 1_000 : clock.remainingMs;
  clock.startedAt = Date.now();
  clock.handle = setTimeout(
    () => {
      clock.handle = null;
      settleClock(clock);
      if (clock.remainingMs <= 0) {
        clock.onExpire();
        return;
      }
      clock.onTick(clock.remainingMs);
      runClock(clock);
    },
    Math.min(delay, clock.remainingMs),
  );
}

function pauseClock(clock: Clock): void {
  if (!clock.handle) return;
  clearTimeout(clock.handle);
  clock.handle = null;
  settleClock(clock);
}

function settleClock(clock: Clock): void {
  if (clock.startedAt === null) return;
  clock.remainingMs = Math.max(0, clock.remainingMs - (Date.now() - clock.startedAt));
  clock.startedAt = null;
}
