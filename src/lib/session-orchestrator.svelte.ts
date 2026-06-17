/**
 * Session Orchestrator — manages session lifecycle, agent states, event listeners, and symphony polling.
 * Tab layout state is delegated to tab-layout.svelte.ts.
 */
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { sessions as sessionsApi, symphony, tasks } from "./api";
import type { Session } from "./types";
import { initSession, getTabCount, destroySession as destroyTabState } from "./session-tabs.svelte";
import { touchMru, getMruList, flushMru, seedMru } from "./mru.svelte";
import { showSnackbar } from "./snackbar.svelte";
import { playTaskComplete } from "./soundPlayer";
import { getCycleState } from "./tab-switcher.svelte";
import {
  activateSession as poolActivate,
  removeSession as poolRemove,
} from "./terminal-pool.svelte";
import {
  cleanup as tabLayoutCleanup,
  resetAll as tabLayoutReset,
  closeShellTab,
} from "./tab-layout.svelte";

// Re-export tab layout functions for consumers still importing from orchestrator
export {
  getDiffTabOpen,
  getDiffTabActive,
  getEditorTabOpen,
  getEditorTabActive,
  getDiffFileName,
  getEditorFileName,
  getEditorModified,
  isEditorModified,
  getUnifiedTabs,
  getUnifiedActiveIndex,
  selectUnifiedTab,
  handleNewTab,
  handleCloseTab,
  handleNextTab,
  handlePrevTab,
  toggleDiff,
  toggleEditor,
  registerEditorRef,
  unregisterEditorRef,
  openFile,
  saveActiveEditor,
  setDiffFileName,
  setEditorFileName,
  setEditorModified,
  closeDiffTab,
  closeEditorTab,
  closeShellTab,
  focusEditorTab,
} from "./tab-layout.svelte";

// ─── State ───────────────────────────────────────────────────────────────────

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);
let agentStates = $state<Record<string, string>>({});

let symphonyStatus = $state<{ active: boolean; slots_used: number; max_concurrent: number } | null>(
  null,
);

// ─── Testing helper ──────────────────────────────────────────────────────────

export function _resetForTests(): void {
  for (const s of sessions) destroyTabState(s.id);
  sessions = [];
  activeSessionId = null;
  agentStates = {};
  symphonyStatus = null;
  tabLayoutReset();
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
export function getAgentStates(): Record<string, string> {
  return agentStates;
}
export function getAgentState(id: string): string | undefined {
  return agentStates[id];
}
export function getSymphonyStatus() {
  return symphonyStatus;
}

// ─── Session Lifecycle ───────────────────────────────────────────────────────

export async function loadSessions(): Promise<void> {
  sessions = await sessionsApi.list();
  for (const s of sessions) {
    if (getTabCount(s.id) === 0) initSession(s.id, s.tab_count);
  }
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

export function selectSession(id: string): void {
  activeSessionId = id;
  poolActivate(id);
  if (agentStates[id] === "Idle") agentStates = { ...agentStates, [id]: "Busy" };
  sessionsApi.acknowledge(id);
}

export function createSession(session: Session): void {
  sessions = [...sessions, session];
  initSession(session.id, 1);
  selectSession(session.id);
}

export async function deleteSession(s: Session): Promise<void> {
  await sessionsApi.destroy(s.id);
  destroyTabState(s.id);
  poolRemove(s.id);
  tabLayoutCleanup(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  if (activeSessionId === s.id) {
    activeSessionId = sessions[0]?.id ?? null;
    if (activeSessionId) poolActivate(activeSessionId);
  }
}

export async function archiveSession(s: Session): Promise<void> {
  await sessionsApi.archive(s.id);
  poolRemove(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  if (activeSessionId === s.id) {
    activeSessionId = sessions[0]?.id ?? null;
    if (activeSessionId) poolActivate(activeSessionId);
  }
}

export async function restartSession(s: Session): Promise<void> {
  const updated = await sessionsApi.restart(s.id);
  sessions = sessions.map((x) => (x.id === s.id ? updated : x));
  selectSession(s.id);
}

export function jumpToSession(index: number): void {
  if (index < sessions.length) selectSession(sessions[index].id);
}

// ─── State setters ───────────────────────────────────────────────────────────

export function clearAgentState(sessionId: string): void {
  const { [sessionId]: _, ...rest } = agentStates;
  agentStates = rest;
}
export function updateSessionStatus(sessionId: string, status: string): void {
  sessions = sessions.map((s) => (s.id === sessionId ? { ...s, status } : s));
}
export function updateSessionName(sessionId: string, name: string): void {
  sessions = sessions.map((s) => (s.id === sessionId ? { ...s, name } : s));
}

export function removeProjectSessions(projectId: string): string[] {
  const ids = sessions.filter((s) => s.project_id === projectId).map((s) => s.id);
  for (const id of ids) {
    poolRemove(id);
    destroyTabState(id);
  }
  sessions = sessions.filter((s) => s.project_id !== projectId);
  if (activeSessionId && ids.includes(activeSessionId)) {
    activeSessionId = getMruList()[0] ?? null;
    if (activeSessionId) poolActivate(activeSessionId);
  }
  return ids;
}

// ─── Event Management ────────────────────────────────────────────────────────

export function startEventListeners(): () => void {
  const unlisteners: Array<Promise<() => void>> = [];

  // Agent state changes (Busy/Idle)
  unlisteners.push(
    listen<{ session_id: string; state: string }>("agent-state-change", (event) => {
      agentStates = { ...agentStates, [event.payload.session_id]: event.payload.state };
      if (event.payload.state === "Idle") {
        playTaskComplete();
        tasks.fireNotifyHook(event.payload.session_id).catch((err) => {
          if (err && typeof err === "string" && err.startsWith("pr_status:")) showSnackbar(err);
        });
      }
    }),
  );

  // Single listener for all PTY exit events (replaces per-session listeners)
  unlisteners.push(
    listen<{ pty_key: string }>("pty-exited", (event) => {
      const { pty_key } = event.payload;
      const colonIdx = pty_key.indexOf(":");
      if (colonIdx !== -1) {
        const sessionId = pty_key.slice(0, colonIdx);
        const tabIndex = parseInt(pty_key.slice(colonIdx + 1), 10);
        closeShellTab(sessionId, tabIndex);
      } else {
        if (!sessions.find((x) => x.id === pty_key)) return;
        sessions = sessions.map((x) => (x.id === pty_key ? { ...x, status: "exited" } : x));
        sessionsApi.markExited(pty_key);
      }
    }),
  );

  // Refresh sessions on PR poll changes
  unlisteners.push(
    listen("sessions-changed", () => {
      if (getCycleState().isCycling) return;
      loadSessions();
    }),
  );

  // Refresh sessions when CLI creates a session
  unlisteners.push(
    listen<string>("session-created", async (event) => {
      await loadSessions();
      touchMru(event.payload);
    }),
  );

  return () => {
    for (const p of unlisteners) p.then((fn) => fn());
  };
}

export function startSymphonyPolling(): () => void {
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

export function getActiveDirectCount(): number {
  return sessions.filter((s) => s.status === "active" && s.backend === "direct").length;
}

export function setupQuitGuard(onShowConfirm: (count: number) => void): Promise<() => void> {
  return getCurrentWindow().onCloseRequested(async (event) => {
    flushMru().catch(() => {});
    const count = getActiveDirectCount();
    if (count > 0) {
      event.preventDefault();
      onShowConfirm(count);
    }
  });
}
