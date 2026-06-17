/**
 * Tab Layout — manages per-session diff/editor tab state and unified tab abstraction.
 * Reads activeSessionId from the session orchestrator (one-way dependency).
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { pty } from "./api";
import type { Tab } from "./session-tabs.svelte";
import { getTabs, addTab, removeTab, setActiveTab, getActiveTabIndex } from "./session-tabs.svelte";
import { getActiveSessionId } from "./session-orchestrator.svelte";

// ─── State ───────────────────────────────────────────────────────────────────

let diffTabOpen = $state<Record<string, boolean>>({});
let diffTabActive = $state<Record<string, boolean>>({});
let editorTabOpen = $state<Record<string, boolean>>({});
let editorTabActive = $state<Record<string, boolean>>({});
let diffFileName = $state<Record<string, string>>({});
let editorFileName = $state<Record<string, string>>({});
let editorModified = $state<Record<string, boolean>>({});
let editorRefs: Record<string, { openFile: (path: string) => void; save: () => void }> = {};

// ─── Getters ─────────────────────────────────────────────────────────────────

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
export function isEditorModified(id: string): boolean {
  return editorModified[id] ?? false;
}

// ─── Cleanup (called by orchestrator on delete/reset) ────────────────────────

export function cleanup(sessionId: string): void {
  const { [sessionId]: _d1, ...restOpen } = diffTabOpen;
  const { [sessionId]: _d2, ...restActive } = diffTabActive;
  const { [sessionId]: _e1, ...edOpen } = editorTabOpen;
  const { [sessionId]: _e2, ...edActive } = editorTabActive;
  const { [sessionId]: _df, ...dfName } = diffFileName;
  const { [sessionId]: _ef, ...efName } = editorFileName;
  const { [sessionId]: _em, ...eMod } = editorModified;
  diffTabOpen = restOpen;
  diffTabActive = restActive;
  editorTabOpen = edOpen;
  editorTabActive = edActive;
  diffFileName = dfName;
  editorFileName = efName;
  editorModified = eMod;
  delete editorRefs[sessionId];
}

export function resetAll(): void {
  diffTabOpen = {};
  diffTabActive = {};
  editorTabOpen = {};
  editorTabActive = {};
  diffFileName = {};
  editorFileName = {};
  editorModified = {};
  editorRefs = {};
}

// ─── Unified Tab Management ──────────────────────────────────────────────────

export function getUnifiedTabs(): Tab[] {
  const id = getActiveSessionId();
  if (!id) return [];
  const shell = getTabs(id);
  const extra: Tab[] = [];
  if (diffTabOpen[id])
    extra.push({ index: -1, label: diffFileName[id] || "Diff", icon: "git-compare" });
  if (editorTabOpen[id])
    extra.push({
      index: -2,
      label: editorFileName[id] || "Editor",
      icon: "file",
      modified: editorModified[id] || false,
    });
  return [...shell, ...extra];
}

export function getUnifiedActiveIndex(): number {
  const id = getActiveSessionId();
  if (!id) return 0;
  if (diffTabActive[id]) return -1;
  if (editorTabActive[id]) return -2;
  return getActiveTabIndex(id);
}

export function selectUnifiedTab(index: number): void {
  const id = getActiveSessionId();
  if (!id) return;
  if (index === -1) {
    diffTabActive = { ...diffTabActive, [id]: true };
    editorTabActive = { ...editorTabActive, [id]: false };
  } else if (index === -2) {
    editorTabActive = { ...editorTabActive, [id]: true };
    diffTabActive = { ...diffTabActive, [id]: false };
  } else {
    diffTabActive = { ...diffTabActive, [id]: false };
    editorTabActive = { ...editorTabActive, [id]: false };
    setActiveTab(id, index);
  }
}

export async function handleNewTab(): Promise<void> {
  const id = getActiveSessionId();
  if (!id) return;
  const tabIndex = addTab(id);
  if (tabIndex === -1) return;
  setActiveTab(id, tabIndex);
  diffTabActive = { ...diffTabActive, [id]: false };
  editorTabActive = { ...editorTabActive, [id]: false };
}

export async function handleCloseTab(): Promise<void> {
  const id = getActiveSessionId();
  if (!id) return;
  if (diffTabActive[id]) {
    diffTabOpen = { ...diffTabOpen, [id]: false };
    diffTabActive = { ...diffTabActive, [id]: false };
    return;
  }
  if (editorTabActive[id]) {
    editorTabOpen = { ...editorTabOpen, [id]: false };
    editorTabActive = { ...editorTabActive, [id]: false };
    return;
  }
  const active = getActiveTabIndex(id);
  if (active === 0) {
    getCurrentWindow().close();
    return;
  }
  await closeShellTab(id, active);
}

export async function closeShellTab(sessionId: string, tabIndex: number): Promise<void> {
  await pty.closeTab(sessionId, tabIndex);
  removeTab(sessionId, tabIndex);
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
  const id = getActiveSessionId();
  if (!id) return;
  if (diffTabOpen[id]) {
    if (diffTabActive[id]) {
      diffTabActive = { ...diffTabActive, [id]: false };
      diffTabOpen = { ...diffTabOpen, [id]: false };
    } else {
      diffTabActive = { ...diffTabActive, [id]: true };
      editorTabActive = { ...editorTabActive, [id]: false };
    }
  } else {
    diffTabOpen = { ...diffTabOpen, [id]: true };
    diffTabActive = { ...diffTabActive, [id]: true };
    editorTabActive = { ...editorTabActive, [id]: false };
  }
}

export function toggleEditor(): void {
  const id = getActiveSessionId();
  if (!id) return;
  if (editorTabOpen[id]) {
    if (editorTabActive[id]) {
      editorTabActive = { ...editorTabActive, [id]: false };
    } else {
      editorTabActive = { ...editorTabActive, [id]: true };
      diffTabActive = { ...diffTabActive, [id]: false };
    }
  } else {
    editorTabOpen = { ...editorTabOpen, [id]: true };
    editorTabActive = { ...editorTabActive, [id]: true };
    diffTabActive = { ...diffTabActive, [id]: false };
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
  const id = getActiveSessionId();
  if (!id) return;
  editorTabOpen = { ...editorTabOpen, [id]: true };
  editorTabActive = { ...editorTabActive, [id]: true };
  diffTabActive = { ...diffTabActive, [id]: false };
  const tryOpen = () => {
    if (editorRefs[id]) editorRefs[id].openFile(filePath);
    else requestAnimationFrame(tryOpen);
  };
  tryOpen();
}

export function saveActiveEditor(): void {
  const id = getActiveSessionId();
  if (id && editorTabActive[id]) editorRefs[id]?.save();
}

// ─── State setters ───────────────────────────────────────────────────────────

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
