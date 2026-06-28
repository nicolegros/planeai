import type { Session } from "./types";

export interface PollerOptions<T> {
  fetch: (sessionId: string) => Promise<T>;
  shouldSkip?: (sessionId: string, current: T | undefined) => boolean;
  onUpdateSessions?: (state: Record<string, T>) => Record<string, T>;
  interval?: number;
}

export interface Poller<T> {
  get: (sessionId: string) => T | undefined;
  refresh: (sessionId: string) => void;
  startPolling: (sessions: Session[]) => () => void;
  updateSessions: (sessions: Session[]) => void;
}

export function createPoller<T>(options: PollerOptions<T>): Poller<T> {
  let state = $state<Record<string, T>>({});
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let activeSessions: Session[] = [];
  const interval = options.interval ?? 60_000;

  async function fetchOne(sessionId: string): Promise<void> {
    try {
      const result = await options.fetch(sessionId);
      state = { ...state, [sessionId]: result };
    } catch {
      // fetch failed, keep previous state
    }
  }

  async function pollAll(): Promise<void> {
    const targets = activeSessions.filter((s) => {
      if (s.status !== "active" || !s.pr_url) return false;
      if (options.shouldSkip?.(s.id, state[s.id])) return false;
      return true;
    });
    if (targets.length === 0) return;
    const results = await Promise.allSettled(
      targets.map(async (s) => ({ id: s.id, value: await options.fetch(s.id) })),
    );
    const next: Record<string, T> = { ...state };
    for (const r of results) {
      if (r.status === "fulfilled") {
        next[r.value.id] = r.value.value;
      }
    }
    state = next;
  }

  return {
    get: (sessionId: string) => state[sessionId],
    refresh: (sessionId: string) => {
      fetchOne(sessionId);
    },
    startPolling: (sessions: Session[]) => {
      activeSessions = sessions;
      pollAll();
      pollTimer = setInterval(pollAll, interval);
      let lastFocusPoll = 0;
      const onFocus = () => {
        const now = Date.now();
        if (now - lastFocusPoll < 5_000) return;
        lastFocusPoll = now;
        pollAll();
      };
      window.addEventListener("focus", onFocus);
      return () => {
        if (pollTimer) {
          clearInterval(pollTimer);
          pollTimer = null;
        }
        window.removeEventListener("focus", onFocus);
      };
    },
    updateSessions: (sessions: Session[]) => {
      activeSessions = sessions;
      if (options.onUpdateSessions) {
        state = options.onUpdateSessions(state);
      }
    },
  };
}
