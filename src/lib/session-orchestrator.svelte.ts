/**
 * Session Orchestrator — extracted from App.svelte.
 * Manages session lifecycle, tab state, agent states, event listeners, and symphony polling.
 * Uses module-level $state (Svelte 5 runes) like session-tabs.svelte.ts and mru.svelte.ts.
 */
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { sessions as sessionsApi, pty, symphony, tasks } from "./api";
import type { Session } from "./types";
import type { Tab } from "./session-tabs.svelte";
import {
  initSession,
  getTabs,
  addTab,
  removeTab,
  setActiveTab,
  getActiveTabIndex,
  getTabCount,
  destroySession as destroyTabState,
} from "./session-tabs.svelte";
import { touchMru, removeMru, getMruList, flushMru, seedMru } from "./mru.svelte";
import { showSnackbar } from "./snackbar.svelte";
import { playTaskComplete } from "./soundPlayer";
import { getCycleState } from "./tab-switcher.svelte";
import {
  activateSession as poolActivate,
  removeSession as poolRemove,
} from "./terminal-pool.svelte";

// ─── State ───────────────────────────────────────────────────────────────────

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);
let agentStates = $state<Record<string, string>>({});

let diffTabOpen = $state<Record<string, boolean>>({});
let diffTabActive = $state<Record<string, boolean>>({});
let editorTabOpen = $state<Record<string, boolean>>({});
let editorTabActive = $state<Record<string, boolean>>({});
let diffFileName = $state<Record<string, string>>({});
let editorFileName = $state<Record<string, string>>({});
let editorModified = $state<Record<string, boolean>>({});

let symphonyStatus = $state<{ active: boolean; slots_used: number; max_concurrent: number } | null>(
  null,
);
let editorRefs: Record<string, { openFile: (path: string) => void; save: () => void }> = {};

// ─── Testing helper ──────────────────────────────────────────────────────────

export function _resetForTests(): void {
  for (const s of sessions) destroyTabState(s.id);
  sessions = [];
  activeSessionId = null;
  agentStates = {};
  diffTabOpen = {};
  diffTabActive = {};
  editorTabOpen = {};
  editorTabActive = {};
  diffFileName = {};
  editorFileName = {};
  editorModified = {};
  symphonyStatus = null;
  editorRefs = {};
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
export function isEditorModified(id: string): boolean {
  return editorModified[id] ?? false;
}
export function getDiffTabOpen(): Record<string, boolean> {
  return diffTabOpen;
}
export function getDiffTabActive(): Record<string, boolean> {
  return diffTabActive;
}
export function getEditorTabOpen(): Record<string, boolean> {
  return editorTabOpen;
}
export function getEditorTabActive(): Record<string, boolean> {
  return editorTabActive;
}
export function getDiffFileName(): Record<string, string> {
  return diffFileName;
}
export function getEditorFileName(): Record<string, string> {
  return editorFileName;
}
export function getEditorModified(): Record<string, boolean> {
  return editorModified;
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
  touchMru(id);
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
  const { [s.id]: _d1, ...restOpen } = diffTabOpen;
  const { [s.id]: _d2, ...restActive } = diffTabActive;
  const { [s.id]: _e1, ...edOpen } = editorTabOpen;
  const { [s.id]: _e2, ...edActive } = editorTabActive;
  const { [s.id]: _df, ...dfName } = diffFileName;
  const { [s.id]: _ef, ...efName } = editorFileName;
  const { [s.id]: _em, ...eMod } = editorModified;
  diffTabOpen = restOpen;
  diffTabActive = restActive;
  editorTabOpen = edOpen;
  editorTabActive = edActive;
  diffFileName = dfName;
  editorFileName = efName;
  editorModified = eMod;
  sessions = sessions.filter((x) => x.id !== s.id);
  removeMru(s.id);
  if (activeSessionId === s.id) {
    activeSessionId = sessions[0]?.id ?? null;
    if (activeSessionId) touchMru(activeSessionId);
  }
}

export async function archiveSession(s: Session): Promise<void> {
  await sessionsApi.archive(s.id);
  poolRemove(s.id);
  sessions = sessions.filter((x) => x.id !== s.id);
  removeMru(s.id);
  if (activeSessionId === s.id) {
    activeSessionId = sessions[0]?.id ?? null;
    if (activeSessionId) touchMru(activeSessionId);
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

// ─── Unified Tab Management ──────────────────────────────────────────────────

export function getUnifiedTabs(): Tab[] {
  if (!activeSessionId) return [];
  const shell = getTabs(activeSessionId);
  const extra: Tab[] = [];
  if (diffTabOpen[activeSessionId])
    extra.push({ index: -1, label: diffFileName[activeSessionId] || "Diff", icon: "git-compare" });
  if (editorTabOpen[activeSessionId])
    extra.push({
      index: -2,
      label: editorFileName[activeSessionId] || "Editor",
      icon: "file",
      modified: editorModified[activeSessionId] || false,
    });
  return [...shell, ...extra];
}

export function getUnifiedActiveIndex(): number {
  if (!activeSessionId) return 0;
  if (diffTabActive[activeSessionId]) return -1;
  if (editorTabActive[activeSessionId]) return -2;
  return getActiveTabIndex(activeSessionId);
}

export function selectUnifiedTab(index: number): void {
  if (!activeSessionId) return;
  if (index === -1) {
    diffTabActive = { ...diffTabActive, [activeSessionId]: true };
    editorTabActive = { ...editorTabActive, [activeSessionId]: false };
  } else if (index === -2) {
    editorTabActive = { ...editorTabActive, [activeSessionId]: true };
    diffTabActive = { ...diffTabActive, [activeSessionId]: false };
  } else {
    diffTabActive = { ...diffTabActive, [activeSessionId]: false };
    editorTabActive = { ...editorTabActive, [activeSessionId]: false };
    setActiveTab(activeSessionId, index);
  }
}

export async function handleNewTab(): Promise<void> {
  if (!activeSessionId) return;
  const tabIndex = addTab(activeSessionId);
  if (tabIndex === -1) return;
  setActiveTab(activeSessionId, tabIndex);
  diffTabActive = { ...diffTabActive, [activeSessionId]: false };
  editorTabActive = { ...editorTabActive, [activeSessionId]: false };
}

export async function handleCloseTab(): Promise<void> {
  if (!activeSessionId) return;
  if (diffTabActive[activeSessionId]) {
    diffTabOpen = { ...diffTabOpen, [activeSessionId]: false };
    diffTabActive = { ...diffTabActive, [activeSessionId]: false };
    return;
  }
  if (editorTabActive[activeSessionId]) {
    editorTabOpen = { ...editorTabOpen, [activeSessionId]: false };
    editorTabActive = { ...editorTabActive, [activeSessionId]: false };
    return;
  }
  const active = getActiveTabIndex(activeSessionId);
  if (active === 0) {
    getCurrentWindow().close();
    return;
  }
  removeTab(activeSessionId, active);
  await pty.closeTab(activeSessionId, active);
}

export function handleNextTab(): void {
  const tabs = getUnifiedTabs();
  if (tabs.length <= 1) return;
  const currentPos = tabs.findIndex((t) => t.index === getUnifiedActiveIndex());
  selectUnifiedTab(tabs[(currentPos + 1) % tabs.length].index);
}

export function handlePrevTab(): void {
  const tabs = getUnifiedTabs();
  if (tabs.length <= 1) return;
  const currentPos = tabs.findIndex((t) => t.index === getUnifiedActiveIndex());
  selectUnifiedTab(tabs[(currentPos - 1 + tabs.length) % tabs.length].index);
}

export function toggleDiff(): void {
  if (!activeSessionId) return;
  if (diffTabOpen[activeSessionId]) {
    if (diffTabActive[activeSessionId]) {
      diffTabActive = { ...diffTabActive, [activeSessionId]: false };
      diffTabOpen = { ...diffTabOpen, [activeSessionId]: false };
    } else {
      diffTabActive = { ...diffTabActive, [activeSessionId]: true };
      editorTabActive = { ...editorTabActive, [activeSessionId]: false };
    }
  } else {
    diffTabOpen = { ...diffTabOpen, [activeSessionId]: true };
    diffTabActive = { ...diffTabActive, [activeSessionId]: true };
    editorTabActive = { ...editorTabActive, [activeSessionId]: false };
  }
}

export function toggleEditor(): void {
  if (!activeSessionId) return;
  if (editorTabOpen[activeSessionId]) {
    if (editorTabActive[activeSessionId]) {
      editorTabActive = { ...editorTabActive, [activeSessionId]: false };
    } else {
      editorTabActive = { ...editorTabActive, [activeSessionId]: true };
      diffTabActive = { ...diffTabActive, [activeSessionId]: false };
    }
  } else {
    editorTabOpen = { ...editorTabOpen, [activeSessionId]: true };
    editorTabActive = { ...editorTabActive, [activeSessionId]: true };
    diffTabActive = { ...diffTabActive, [activeSessionId]: false };
  }
}

// ─── Editor Integration ──────────────────────────────────────────────────────

export function registerEditorRef(
  sessionId: string,
  ref: { openFile: (path: string) => void; save: () => void },
): void {
  editorRefs[sessionId] = ref;
}
export function unregisterEditorRef(sessionId: string): void {
  delete editorRefs[sessionId];
}

export function openFile(filePath: string): void {
  if (!activeSessionId) return;
  editorTabOpen = { ...editorTabOpen, [activeSessionId]: true };
  editorTabActive = { ...editorTabActive, [activeSessionId]: true };
  diffTabActive = { ...diffTabActive, [activeSessionId]: false };
  const sid = activeSessionId;
  const tryOpen = () => {
    if (editorRefs[sid]) editorRefs[sid].openFile(filePath);
    else requestAnimationFrame(tryOpen);
  };
  tryOpen();
}

export function saveActiveEditor(): void {
  if (activeSessionId && editorTabActive[activeSessionId]) editorRefs[activeSessionId]?.save();
}

// ─── State setters (called from template) ────────────────────────────────────

export function setDiffFileName(sessionId: string, name: string): void {
  diffFileName = { ...diffFileName, [sessionId]: name };
}
export function setEditorFileName(sessionId: string, name: string): void {
  editorFileName = { ...editorFileName, [sessionId]: name };
}
export function setEditorModified(sessionId: string, modified: boolean): void {
  editorModified = { ...editorModified, [sessionId]: modified };
}
export function closeDiffTab(sessionId: string): void {
  diffTabOpen = { ...diffTabOpen, [sessionId]: false };
  diffTabActive = { ...diffTabActive, [sessionId]: false };
}
export function closeEditorTab(sessionId: string): void {
  editorTabOpen = { ...editorTabOpen, [sessionId]: false };
  editorTabActive = { ...editorTabActive, [sessionId]: false };
}
export function focusEditorTab(sessionId: string): void {
  editorTabActive = { ...editorTabActive, [sessionId]: true };
  diffTabActive = { ...diffTabActive, [sessionId]: false };
}
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
    removeMru(id);
    destroyTabState(id);
    poolRemove(id);
  }
  sessions = sessions.filter((s) => s.project_id !== projectId);
  if (activeSessionId && ids.includes(activeSessionId)) {
    activeSessionId = getMruList()[0] ?? null;
    if (activeSessionId) touchMru(activeSessionId);
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
        removeTab(sessionId, tabIndex);
        pty.closeTab(sessionId, tabIndex);
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
