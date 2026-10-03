/**
 * Session Orchestrator — manages session lifecycle, agent states, event listeners, and symphony polling.
 */
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { sessions as sessionsApi, symphony, tasks } from "./api";
import { sessionTaskProjectId, type Session } from "./types";
import { touchMru, removeMru, getMruList, flushMru, seedMru } from "./mru.svelte";
import { disposeSessionTerminalViews } from "./terminal-views";
import { clearComments } from "./review-comments.svelte";
import { clearEditorFeedback } from "./editor-feedback.svelte";
import { destroySession as destroyViewedState } from "./diff-viewed.svelte";
import { showSnackbar } from "./snackbar.svelte";
import { dismissForSession } from "./post-merge-prompt.svelte";
import { getSettings } from "./settings.svelte";
import { playTaskComplete } from "./soundPlayer";
import { getCycleState } from "./tab-switcher.svelte";
import { taskWorkspaceLayout } from "./task-workspace-layout.svelte";
import { PROVIDER_BACKEND } from "./plugin-providers";

// ─── State ───────────────────────────────────────────────────────────────────

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);
let agentStates = $state<Record<string, string>>({});
/** Sessions whose agent is mid-turn. Unlike `agentStates`, selecting a session does not clear it. */
const turns = new Set<string>();

let symphonyStatus = $state<{ active: boolean; slots_used: number; max_concurrent: number } | null>(
  null,
);
let reviewReady = $state<Record<string, boolean>>({});

// ─── Testing helper ──────────────────────────────────────────────────────────

export function _resetForTests(): void {
  sessions = [];
  setActiveSession(null);
  agentStates = {};
  turns.clear();
  symphonyStatus = null;
  reviewReady = {};
}

export function _setReviewReadyForTests(sessionId: string): void {
  reviewReady = { ...reviewReady, [sessionId]: true };
}

// ─── Getters ─────────────────────────────────────────────────────────────────

export function getSessions(): Session[] {
  return sessions;
}
export function getActiveSessionId(): string | null {
  return activeSessionId;
}
export function getActiveSession(): Session | undefined {
  return sessions.find((s) => s.id === activeSessionId);
}
/** Session IDs that are valid switch targets (excludes exited sessions). */
export function getSwitchableSessionIds(): Set<string> {
  return new Set(sessions.filter((s) => s.status !== "exited").map((s) => s.id));
}
export function getAgentStates(): Record<string, string> {
  return agentStates;
}
export function getAgentState(id: string): string | undefined {
  return agentStates[id];
}
export function getSymphonyStatus() {
  return symphonyStatus;
}
export function getReviewReady(): Record<string, boolean> {
  return reviewReady;
}
export function clearReviewReady(sessionId: string): void {
  if (!reviewReady[sessionId]) return;
  const { [sessionId]: _, ...rest } = reviewReady;
  reviewReady = rest;
}

// ─── Session Lifecycle ───────────────────────────────────────────────────────

export async function loadSessions(): Promise<void> {
  const loadedSessions = await sessionsApi.list();
  // Sessions can leave outside this module (e.g. CLI archive); a lingering view
  // would be reused with a dead connection if the session came back.
  const loadedIds = new Set(loadedSessions.map((s) => s.id));
  for (const s of sessions) if (!loadedIds.has(s.id)) removeSessionViews(s.id);
  sessions = loadedSessions;
  if (sessions.length > 0 && !activeSessionId) {
    seedMru(sessions.map((s) => s.id));
    selectSession(sessions[0].id);
  } else {
    const mru = getMruList();
    for (const s of sessions) {
      if (!mru.includes(s.id)) touchMru(s.id);
    }
  }
}

/**
 * Session the user explicitly asked for, as opposed to an arbitrary entry point
 * such as boot or the post-delete fallback. A restored workspace layout keeps its
 * remembered tab unless the selection was explicit.
 */
let explicitlySelectedSessionId = $state<string | null>(null);

export function isSelectionExplicit(sessionId: string): boolean {
  return explicitlySelectedSessionId === sessionId;
}

/**
 * Sole writer of the active session, so the explicit mark is always either the
 * current selection or absent. Fallbacks (delete/archive/park/project removal)
 * pass no options and therefore clear it.
 */
function setActiveSession(id: string | null, opts: { explicit?: boolean } = {}): void {
  explicitlySelectedSessionId = opts.explicit && id ? id : null;
  activeSessionId = id;
}

export function selectSession(id: string, opts: { explicit?: boolean } = {}): void {
  console.log(`[DEBUG-lsr1] selectSession called`, {
    id,
    foundInSessions: sessions.some((s) => s.id === id),
    sessionCount: sessions.length,
    timestamp: Date.now(),
  });
  setActiveSession(id, opts);
  const session = sessions.find((s) => s.id === id);
  if (session?.status === "exited") {
    // The session stays `exited` until restart resolves, so its terminal view
    // does not attach to the still-exited daemon session (immediate EOF, then
    // pty-exited again: a loop).
    sessionsApi
      .restart(id)
      .then((updated) => {
        if (updated) sessions = sessions.map((x) => (x.id === id ? updated : x));
        if (activeSessionId === id) touchMru(id);
      })
      .catch((e) => {
        showSnackbar(`Restart failed: ${e}`);
        if (activeSessionId === id) touchMru(id);
      });
  } else {
    touchMru(id);
  }
  if (agentStates[id]) {
    clearAgentState(id);
  } else {
    sessionsApi.acknowledge(id);
  }
}

export function createSession(session: Session): void {
  // Replace rather than append: a `sessions-changed` reload can already have
  // fetched this session, and a duplicate entry produces duplicate tab keys,
  // which is a Svelte runtime error that freezes the UI.
  const existing = sessions.findIndex((candidate) => candidate.id === session.id);
  sessions =
    existing === -1
      ? [...sessions, session]
      : sessions.map((candidate) => (candidate.id === session.id ? session : candidate));
  selectSession(session.id, { explicit: true });
}

/** A session leaving the app takes its terminal views (agent and shells) with it. */
function removeSessionViews(sessionId: string): void {
  removeMru(sessionId);
  disposeSessionTerminalViews(sessionId);
}

export async function deleteSession(s: Session): Promise<void> {
  await sessionsApi.destroy(s.id);
  dismissForSession(s.id);
  clearComments(s.id);
  clearEditorFeedback(s.id);
  destroyViewedState(s.id);
  removeSessionViews(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  if (activeSessionId === s.id) {
    setActiveSession(sessions[0]?.id ?? null);
    if (activeSessionId) touchMru(activeSessionId);
  }
}

export async function archiveSession(s: Session): Promise<void> {
  await sessionsApi.archive(s.id);
  dismissForSession(s.id);
  clearComments(s.id);
  clearEditorFeedback(s.id);
  destroyViewedState(s.id);
  removeSessionViews(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  if (activeSessionId === s.id) {
    setActiveSession(sessions[0]?.id ?? null);
    if (activeSessionId) touchMru(activeSessionId);
  }
}

export async function parkSession(s: Session): Promise<void> {
  await sessionsApi.park(s.id);
  dismissForSession(s.id);
  clearComments(s.id);
  clearEditorFeedback(s.id);
  destroyViewedState(s.id);
  removeSessionViews(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  if (activeSessionId === s.id) {
    setActiveSession(
      sessions.find(
        (x) => sessionTaskProjectId(x) === sessionTaskProjectId(s) && x.task_key === s.task_key,
      )?.id ?? null,
    );
    if (activeSessionId) touchMru(activeSessionId);
  }
}

export async function restartSession(s: Session): Promise<void> {
  const updated = await sessionsApi.restart(s.id);
  sessions = sessions.map((x) => (x.id === s.id ? updated : x));
  selectSession(s.id, { explicit: true });
}

export function jumpToSession(index: number): void {
  if (index < sessions.length) selectSession(sessions[index].id, { explicit: true });
}

// ─── State setters ───────────────────────────────────────────────────────────

export function clearAgentState(sessionId: string): void {
  const { [sessionId]: _, ...rest } = agentStates;
  agentStates = rest;
  sessionsApi.acknowledge(sessionId);
}

/** Invalidate review state before bytes from a user action reach the PTY. */
export function recordUserInput(sessionId: string): void {
  if (agentStates[sessionId]) clearAgentState(sessionId);
  clearReviewReady(sessionId);
}
export function updateSessionStatus(sessionId: string, status: Session["status"]): void {
  sessions = sessions.map((s) => (s.id === sessionId ? { ...s, status } : s));
}
export function updateSessionName(sessionId: string, name: string): void {
  sessions = sessions.map((s) => (s.id === sessionId ? { ...s, name } : s));
}

export function removeProjectSessions(projectId: string): string[] {
  const ids = sessions.filter((s) => s.project_id === projectId).map((s) => s.id);
  for (const id of ids) {
    removeSessionViews(id);
    clearEditorFeedback(id);
  }
  sessions = sessions.filter((s) => s.project_id !== projectId);
  if (activeSessionId && ids.includes(activeSessionId)) {
    setActiveSession(getMruList()[0] ?? null);
    if (activeSessionId) touchMru(activeSessionId);
  }
  return ids;
}

// ─── Event Management ────────────────────────────────────────────────────────

/** A session's hooks run in event order, so a quick resume cannot overtake the notify move it reverts. */
const taskHookQueues = new Map<string, Promise<unknown>>();
function queueTaskHook(sessionId: string, fire: (sessionId: string) => Promise<unknown>): void {
  const next = (taskHookQueues.get(sessionId) ?? Promise.resolve())
    .then(() => fire(sessionId))
    .catch(() => {});
  taskHookQueues.set(sessionId, next);
  void next.then(() => {
    if (taskHookQueues.get(sessionId) === next) taskHookQueues.delete(sessionId);
  });
}

export function startEventListeners(): () => void {
  const unlisteners: Array<Promise<() => void>> = [];

  // Emitted only when the agent's own hook reports work after a known idle.
  unlisteners.push(
    listen<{ session_id: string }>("agent-resumed", (event) => {
      queueTaskHook(event.payload.session_id, tasks.fireResumeHook);
    }),
  );

  // A provider's sidecar went away mid-turn: the turn ended without finishing.
  unlisteners.push(
    listen<{ session_id: string }>("agent-released", (event) => {
      const id = event.payload.session_id;
      turns.delete(id);
      if (agentStates[id] === "Busy") clearAgentState(id);
    }),
  );

  // Agent state changes (Busy/Idle)
  unlisteners.push(
    listen<{ session_id: string; state: string }>("agent-state-change", (event) => {
      agentStates = { ...agentStates, [event.payload.session_id]: event.payload.state };
      if (event.payload.state === "Busy") turns.add(event.payload.session_id);
      else turns.delete(event.payload.session_id);
      if (event.payload.state === "Idle") {
        if (getSettings().sound_enabled !== false) {
          playTaskComplete();
        }
        queueTaskHook(event.payload.session_id, tasks.fireNotifyHook);
        // Auto-open review tab when agent finishes
        const sid = event.payload.session_id;
        const session = sessions.find((s) => s.id === sid);
        if (session?.worktree_path && session.base_branch) {
          if (sid === activeSessionId) {
            if (getSettings().auto_open_review === true) {
              // Defer to next frame so state updates don't block the current tick
              requestAnimationFrame(() => taskWorkspaceLayout.openDiff(sid));
            }
          } else {
            reviewReady = { ...reviewReady, [sid]: true };
          }
        }
      }
    }),
  );

  // Single listener for all PTY exit events (replaces per-session listeners)
  unlisteners.push(
    listen<{ pty_key: string }>("pty-exited", (event) => {
      const { pty_key } = event.payload;
      if (pty_key.includes(":")) return;
      if (!sessions.find((x) => x.id === pty_key)) return;
      sessions = sessions.map((x) => (x.id === pty_key ? { ...x, status: "exited" } : x));
      sessionsApi.markExited(pty_key);
    }),
  );

  // Refresh sessions on PR poll changes
  unlisteners.push(
    listen("sessions-changed", () => {
      if (getCycleState().isCycling) return;
      loadSessions();
    }),
  );

  // Re-attach PTY when a session is restarted
  unlisteners.push(
    listen<{ session_id: string }>("session-restarted", async (event) => {
      const { session_id } = event.payload;
      sessions = sessions.map((x) => (x.id === session_id ? { ...x, status: "active" } : x));
    }),
  );

  // Refresh sessions when CLI creates a session
  unlisteners.push(
    listen<string>("session-created", async (event) => {
      console.log(`[DEBUG-lsr1] session-created event received`, {
        sessionId: event.payload,
        timestamp: Date.now(),
      });
      await loadSessions();
      console.log(`[DEBUG-lsr1] loadSessions() resolved after session-created`, {
        sessionId: event.payload,
        sessionCount: sessions.length,
        found: sessions.some((s) => s.id === event.payload),
        timestamp: Date.now(),
      });
      touchMru(event.payload);
    }),
  );

  return () => {
    for (const p of unlisteners) p.then((fn) => fn());
  };
}

export function startSymphonyPolling(): () => void {
  const settings = getSettings();
  if (!settings.task_management?.auto_dispatch) {
    return () => {};
  }
  const poll = async () => {
    try {
      symphonyStatus = JSON.parse(await symphony.getStatus());
    } catch {
      symphonyStatus = null;
    }
  };
  poll();
  const id = setInterval(poll, 5000);
  return () => clearInterval(id);
}

// ─── Quit confirmation helper ────────────────────────────────────────────────

/** Local PTYs die with the app, and so does a provider session's in-flight turn. */
export function runningTurns(): ReadonlySet<string> {
  return turns;
}

export function countSessionsLostOnQuit(
  candidates: Pick<Session, "id" | "status" | "backend">[],
  running: ReadonlySet<string>,
): number {
  return candidates.filter(
    (s) =>
      s.status === "active" &&
      (s.backend === "local" || (s.backend === PROVIDER_BACKEND && running.has(s.id))),
  ).length;
}

export function setupQuitGuard(onShowConfirm: (count: number) => void): Promise<() => void> {
  return getCurrentWindow().onCloseRequested(async (event) => {
    flushMru().catch(() => {});
    const count = countSessionsLostOnQuit(sessions, turns);
    if (count > 0) {
      event.preventDefault();
      onShowConfirm(count);
    }
  });
}
