import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    close: vi.fn(),
    onCloseRequested: vi.fn(() => Promise.resolve(() => {})),
  })),
}));
vi.mock("../snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../soundPlayer", () => ({ playTaskComplete: vi.fn() }));
vi.mock("../settings.svelte", () => ({
  getSettings: vi.fn(() => ({
    appearance: { mode: "system", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {},
    default_provider: "kiro",
    task_management: null,
  })),
}));
vi.mock("../tab-switcher.svelte", () => ({
  getCycleState: vi.fn(() => ({ isCycling: false, cycleList: [], index: 0, isVisible: false })),
}));
vi.mock("../mru.svelte", () => ({
  activateSession: vi.fn(),
  removeSession: vi.fn(),
  touchMru: vi.fn(),
  getMruList: vi.fn(() => []),
  flushMru: vi.fn(() => Promise.resolve()),
  seedMru: vi.fn(),
}));

vi.mock("../api", () => ({
  sessions: {
    list: vi.fn(() => Promise.resolve([])),
    destroy: vi.fn(() => Promise.resolve()),
    archive: vi.fn(() => Promise.resolve()),
    park: vi.fn(() => Promise.resolve()),
    restart: vi.fn(() => Promise.resolve()),
    markExited: vi.fn(),
    acknowledge: vi.fn(() => Promise.resolve()),
    saveMruOrder: vi.fn(() => Promise.resolve()),
  },
  pty: { closeTab: vi.fn(() => Promise.resolve()) },
  symphony: { getStatus: vi.fn(() => Promise.resolve("null")) },
  tasks: { fireNotifyHook: vi.fn(() => Promise.resolve()) },
  git: { getChangedFiles: vi.fn(() => Promise.resolve([])) },
}));

import { sessions as sessionsApi, symphony } from "../api";
import * as splitTree from "../split-tree.svelte";
import { getSettings } from "../settings.svelte";
import type { Session } from "../types";
import {
  addEditorFeedback,
  getEditorFeedbackCount,
  _resetForTests as resetEditorFeedback,
} from "../editor-feedback.svelte";
import {
  getSessions,
  getActiveSessionId,
  loadSessions,
  selectSession,
  createSession,
  deleteSession,
  archiveSession,
  parkSession,
  removeProjectSessions,
  restartSession,
  jumpToSession,
  isSelectionExplicit,
  getUnifiedTabs,
  getUnifiedActiveIndex,
  selectUnifiedTab,
  handleNextTab,
  handlePrevTab,
  toggleEditor,
  getEditorTabOpen,
  getEditorTabActive,
  getAgentStates,
  clearAgentState,
  recordUserInput,
  getReviewReady,
  clearReviewReady,
  startEventListeners,
  startSymphonyPolling,
  _resetForTests,
  _setReviewReadyForTests,
} from "../session-orchestrator.svelte";

const api = vi.mocked(sessionsApi);

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    project_id: "p1",
    name: "test",
    tmux_name: null,
    branch: "main",
    status: "active",
    created_at: "2024-01-01",
    worktree_path: null,
    provider: "kiro",
    backend: "tmux",
    tab_count: 1,
    base_branch: null,
    task_key: null,
    ...overrides,
  };
}

describe("session-orchestrator", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    _resetForTests();
    resetEditorFeedback();
    api.list.mockResolvedValue([]);
  });

  const queuedFeedback = {
    filePath: "src/example.ts",
    startLine: 1,
    endLine: 1,
    language: "typescript",
    selectedText: "const answer = 42;",
    contextBefore: "",
    contextAfter: "",
    isUnsaved: false,
    text: "Please revise this.",
  };

  function queueEditorFeedback(sessionId: string): void {
    addEditorFeedback(sessionId, queuedFeedback);
  }

  describe("loadSessions", () => {
    it("populates sessions from API", async () => {
      const s1 = makeSession({ id: "s1" }),
        s2 = makeSession({ id: "s2", name: "two" });

      api.list.mockResolvedValue([s1, s2]);
      await loadSessions();
      expect(getSessions()).toEqual([s1, s2]);
      expect(getActiveSessionId()).toBe("s1");
    });

    it("selects first session when no active session", async () => {
      api.list.mockResolvedValue([makeSession({ id: "abc" })]);
      await loadSessions();
      expect(getActiveSessionId()).toBe("abc");
    });
  });

  describe("selectSession", () => {
    it("sets activeSessionId and acknowledges on backend", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" }), makeSession({ id: "s2" })]);
      await loadSessions();
      selectSession("s2");
      expect(getActiveSessionId()).toBe("s2");
      expect(api.acknowledge).toHaveBeenCalledWith("s2");
    });
  });

  describe("createSession", () => {
    it("appends session to list and selects it", async () => {
      api.list.mockResolvedValue([]);
      await loadSessions();
      const s = makeSession({ id: "new1", name: "new" });
      createSession(s);
      expect(getSessions()).toContainEqual(s);
      expect(getActiveSessionId()).toBe("new1");
    });

    it("does not duplicate a session a concurrent reload already fetched", async () => {
      // A `sessions-changed` reload can land between launch and createSession.
      // A duplicated session yields duplicate tab keys, and a duplicate key in a
      // Svelte keyed `{#each}` aborts the update flush — freezing the whole UI,
      // which looks like session selection doing nothing.
      const existing = makeSession({ id: "dup1", name: "from reload" });
      api.list.mockResolvedValue([existing]);
      await loadSessions();

      createSession(makeSession({ id: "dup1", name: "from launch" }));

      const matching = getSessions().filter((session) => session.id === "dup1");
      expect(matching).toHaveLength(1);
      // The newer object wins, so the caller's data is not discarded.
      expect(matching[0].name).toBe("from launch");
      expect(getActiveSessionId()).toBe("dup1");
    });
  });

  describe("deleteSession", () => {
    it("removes session from list and calls API", async () => {
      const s1 = makeSession({ id: "s1" }),
        s2 = makeSession({ id: "s2" });
      api.list.mockResolvedValue([s1, s2]);
      await loadSessions();
      await deleteSession(s1);
      expect(getSessions()).not.toContainEqual(s1);
      expect(api.destroy).toHaveBeenCalledWith("s1");
    });

    it("clears queued editor feedback", async () => {
      const s1 = makeSession({ id: "s1" });
      api.list.mockResolvedValue([s1]);
      await loadSessions();
      queueEditorFeedback(s1.id);

      await deleteSession(s1);

      expect(getEditorFeedbackCount(s1.id)).toBe(0);
    });

    it("selects next session when active is deleted", async () => {
      const s1 = makeSession({ id: "s1" }),
        s2 = makeSession({ id: "s2" });
      api.list.mockResolvedValue([s1, s2]);
      await loadSessions();
      selectSession("s1");
      await deleteSession(s1);
      expect(getActiveSessionId()).toBe("s2");
    });
  });

  describe("archiveSession", () => {
    it("removes session from list", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      await archiveSession(makeSession({ id: "s1" }));
      expect(getSessions()).toHaveLength(0);
      expect(api.archive).toHaveBeenCalledWith("s1");
    });

    it("clears queued editor feedback", async () => {
      const s1 = makeSession({ id: "s1" });
      api.list.mockResolvedValue([s1]);
      await loadSessions();
      queueEditorFeedback(s1.id);

      await archiveSession(s1);

      expect(getEditorFeedbackCount(s1.id)).toBe(0);
    });
  });

  describe("restartSession", () => {
    describe("parkSession", () => {
      it("uses the non-completing endpoint and keeps a same-task agent selected", async () => {
        const parked = makeSession({ id: "s1", task_key: "PLA-315" });
        const sibling = makeSession({ id: "s2", task_key: "PLA-315" });
        api.list.mockResolvedValue([parked, sibling]);
        await loadSessions();
        selectSession(parked.id);

        await parkSession(parked);

        expect(api.park).toHaveBeenCalledWith(parked.id);
        expect(api.archive).not.toHaveBeenCalled();
        expect(getSessions().map((session) => session.id)).toEqual([sibling.id]);
        expect(getActiveSessionId()).toBe(sibling.id);
      });

      it("allows the final task-linked agent to park without selecting another session", async () => {
        const parked = makeSession({ id: "s1", task_key: "PLA-315" });
        api.list.mockResolvedValue([parked]);
        await loadSessions();

        await parkSession(parked);

        expect(getSessions()).toEqual([]);
        expect(getActiveSessionId()).toBeNull();
      });
    });
    it("replaces session with updated version", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", status: "exited" })]);
      await loadSessions();
      api.restart.mockResolvedValue(makeSession({ id: "s1", status: "active" }));
      await restartSession(makeSession({ id: "s1", status: "exited" }));
      expect(getSessions().find((s) => s.id === "s1")?.status).toBe("active");
    });
  });

  describe("removeProjectSessions", () => {
    it("removes every project session", async () => {
      api.list.mockResolvedValue([
        makeSession({ id: "p1-a", project_id: "p1" }),
        makeSession({ id: "p1-b", project_id: "p1" }),
        makeSession({ id: "p2-a", project_id: "p2" }),
      ]);
      await loadSessions();

      expect(removeProjectSessions("p1")).toEqual(["p1-a", "p1-b"]);
      expect(getSessions().map((session) => session.id)).toEqual(["p2-a"]);
    });

    it("clears queued feedback for every removed project session", async () => {
      api.list.mockResolvedValue([
        makeSession({ id: "p1-a", project_id: "p1" }),
        makeSession({ id: "p1-b", project_id: "p1" }),
        makeSession({ id: "p2-a", project_id: "p2" }),
      ]);
      await loadSessions();
      queueEditorFeedback("p1-a");
      queueEditorFeedback("p1-b");
      queueEditorFeedback("p2-a");

      removeProjectSessions("p1");

      expect(getEditorFeedbackCount("p1-a")).toBe(0);
      expect(getEditorFeedbackCount("p1-b")).toBe(0);
      expect(getEditorFeedbackCount("p2-a")).toBe(1);
    });
  });

  describe("selectSession exited daemon fix (PLA-169)", () => {
    it("waits for restart before activating pool for exited sessions", async () => {
      const { activateSession: poolActivate } = await import("../mru.svelte");
      const poolMock = vi.mocked(poolActivate);

      api.list.mockResolvedValue([
        makeSession({ id: "s1", status: "active" }),
        makeSession({ id: "s2", status: "exited", backend: "daemon" }),
      ]);
      api.restart.mockResolvedValue(makeSession({ id: "s2", status: "active", backend: "daemon" }));
      await loadSessions();

      poolMock.mockClear();
      selectSession("s2");

      // Pool should NOT be activated synchronously for exited sessions
      expect(poolMock).not.toHaveBeenCalled();

      // After restart resolves, pool is activated
      await vi.waitFor(() => expect(poolMock).toHaveBeenCalledWith("s2"));
    });

    it("activates pool immediately for active sessions", async () => {
      const { activateSession: poolActivate } = await import("../mru.svelte");
      const poolMock = vi.mocked(poolActivate);

      api.list.mockResolvedValue([makeSession({ id: "s1", status: "active" })]);
      await loadSessions();

      poolMock.mockClear();
      selectSession("s1");

      // Active sessions activate pool immediately
      expect(poolMock).toHaveBeenCalledWith("s1");
    });
  });

  describe("unified tab cycling", () => {
    it("getUnifiedTabs returns shell tabs", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", tab_count: 2 })]);
      await loadSessions();
      expect(getUnifiedTabs().length).toBe(2);
    });

    it("selectUnifiedTab changes active", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", tab_count: 3 })]);
      await loadSessions();
      selectUnifiedTab(2);
      expect(getUnifiedActiveIndex()).toBe(2);
    });

    it("handleNextTab cycles forward", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", tab_count: 3 })]);
      await loadSessions();
      handleNextTab();
      expect(getUnifiedActiveIndex()).toBe(1);
      handleNextTab();
      expect(getUnifiedActiveIndex()).toBe(2);
      handleNextTab();
      expect(getUnifiedActiveIndex()).toBe(0);
    });

    it("handlePrevTab cycles backward", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", tab_count: 3 })]);
      await loadSessions();
      handlePrevTab();
      expect(getUnifiedActiveIndex()).toBe(2);
      handlePrevTab();
      expect(getUnifiedActiveIndex()).toBe(1);
    });

    it("toggleEditor opens and activates", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      toggleEditor();
      expect(getEditorTabOpen()["s1"]).toBe(true);
      expect(getEditorTabActive()["s1"]).toBe(true);
    });
  });

  describe("event management", () => {
    it("startEventListeners returns cleanup", () => {
      const cleanup = startEventListeners();
      expect(typeof cleanup).toBe("function");
      cleanup();
    });

    it("does not play sound when sound_enabled is false", async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const listenMock = vi.mocked(listen);
      listenMock.mockClear();

      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
        sound_enabled: false,
      });

      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();

      const cleanup = startEventListeners();
      // Find the agent-state-change listener callback
      const agentCall = listenMock.mock.calls.find((c) => c[0] === "agent-state-change");
      expect(agentCall).toBeDefined();
      const handler = agentCall![1] as (event: {
        payload: { session_id: string; state: string };
      }) => void;

      const { playTaskComplete } = await import("../soundPlayer");
      vi.mocked(playTaskComplete).mockClear();

      handler({ payload: { session_id: "s1", state: "Idle" } });
      expect(playTaskComplete).not.toHaveBeenCalled();

      cleanup();
    });

    it("plays sound when sound_enabled is true", async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const listenMock = vi.mocked(listen);
      listenMock.mockClear();

      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
        sound_enabled: true,
      });

      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();

      const cleanup = startEventListeners();
      const agentCall = listenMock.mock.calls.find((c) => c[0] === "agent-state-change");
      expect(agentCall).toBeDefined();
      const handler = agentCall![1] as (event: {
        payload: { session_id: string; state: string };
      }) => void;

      const { playTaskComplete } = await import("../soundPlayer");
      vi.mocked(playTaskComplete).mockClear();

      handler({ payload: { session_id: "s1", state: "Idle" } });
      expect(playTaskComplete).toHaveBeenCalled();

      cleanup();
    });

    it("plays sound when sound_enabled is undefined (defaults to on)", async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const listenMock = vi.mocked(listen);
      listenMock.mockClear();

      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
      });

      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();

      const cleanup = startEventListeners();
      const agentCall = listenMock.mock.calls.find((c) => c[0] === "agent-state-change");
      expect(agentCall).toBeDefined();
      const handler = agentCall![1] as (event: {
        payload: { session_id: string; state: string };
      }) => void;

      const { playTaskComplete } = await import("../soundPlayer");
      vi.mocked(playTaskComplete).mockClear();

      handler({ payload: { session_id: "s1", state: "Idle" } });
      expect(playTaskComplete).toHaveBeenCalled();

      cleanup();
    });

    it("auto-opens the review tab in the workspace layout when the active agent finishes", async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const listenMock = vi.mocked(listen);
      listenMock.mockClear();

      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
        sound_enabled: false,
      });

      api.list.mockResolvedValue([
        makeSession({ id: "s1", worktree_path: "/tmp/wt", base_branch: "main" }),
      ]);
      await loadSessions();
      selectSession("s1");

      splitTree.resetTree();
      splitTree.initTree([{ ptyKey: "s1", label: "Agent", icon: "bot", type: "agent" }], "s1");

      const cleanup = startEventListeners();
      const agentCall = listenMock.mock.calls.find((c) => c[0] === "agent-state-change");
      const handler = agentCall![1] as (event: {
        payload: { session_id: string; state: string };
      }) => void;

      handler({ payload: { session_id: "s1", state: "Idle" } });
      await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

      const leaf = splitTree.getFocusedLeaf();
      expect(leaf?.tabs.map((tab) => tab.ptyKey)).toEqual(["s1", "s1:diff"]);
      expect(leaf?.activeTab).toBe("s1:diff");

      cleanup();
    });

    it("does not close an already-open review tab when the agent finishes again", async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const listenMock = vi.mocked(listen);
      listenMock.mockClear();

      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
        sound_enabled: false,
      });

      api.list.mockResolvedValue([
        makeSession({ id: "s1", worktree_path: "/tmp/wt", base_branch: "main" }),
      ]);
      await loadSessions();
      selectSession("s1");

      splitTree.resetTree();
      splitTree.initTree([{ ptyKey: "s1", label: "Agent", icon: "bot", type: "agent" }], "s1");

      const cleanup = startEventListeners();
      const agentCall = listenMock.mock.calls.find((c) => c[0] === "agent-state-change");
      const handler = agentCall![1] as (event: {
        payload: { session_id: string; state: string };
      }) => void;

      handler({ payload: { session_id: "s1", state: "Idle" } });
      await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
      handler({ payload: { session_id: "s1", state: "Idle" } });
      await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

      expect(splitTree.getFocusedLeaf()?.tabs.map((tab) => tab.ptyKey)).toEqual(["s1", "s1:diff"]);

      cleanup();
    });

    it("startSymphonyPolling returns cleanup", () => {
      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: { auto_dispatch: { max_concurrent: 2 } },
      });
      vi.mocked(symphony.getStatus).mockResolvedValue(
        JSON.stringify({ active: true, slots_used: 1, max_concurrent: 3 }),
      );
      const cleanup = startSymphonyPolling();
      expect(typeof cleanup).toBe("function");
      expect(symphony.getStatus).toHaveBeenCalled();
      cleanup();
    });

    it("startSymphonyPolling skips polling when auto_dispatch is not configured", () => {
      vi.mocked(getSettings).mockReturnValue({
        appearance: { mode: "system", theme: "default" },
        terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
        providers: {},
        default_provider: "kiro",
        task_management: null,
      });
      vi.mocked(symphony.getStatus).mockClear();
      const cleanup = startSymphonyPolling();
      expect(typeof cleanup).toBe("function");
      expect(symphony.getStatus).not.toHaveBeenCalled();
      cleanup();
    });
  });

  describe("clearAgentState", () => {
    it("removes agent state", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      clearAgentState("s1");
      expect(getAgentStates()["s1"]).toBeUndefined();
    });
  });

  describe("reviewReady (PLA-187)", () => {
    it("selectSession does NOT clear reviewReady (bulb persists until user input)", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" }), makeSession({ id: "s2" })]);
      await loadSessions();
      _setReviewReadyForTests("s2");
      expect(getReviewReady()["s2"]).toBe(true);

      selectSession("s2");
      expect(getReviewReady()["s2"]).toBe(true);
    });

    it("clearReviewReady removes the bulb for a session", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" }), makeSession({ id: "s2" })]);
      await loadSessions();
      _setReviewReadyForTests("s1");
      _setReviewReadyForTests("s2");

      clearReviewReady("s2");
      expect(getReviewReady()["s2"]).toBeUndefined();
      expect(getReviewReady()["s1"]).toBe(true);
    });

    it("recordUserInput clears review readiness before a write", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      _setReviewReadyForTests("s1");

      recordUserInput("s1");

      expect(getReviewReady()["s1"]).toBeUndefined();
    });
  });
});

describe("selection explicitness", () => {
  // Without its own reset this suite would run against state leaked from the
  // suite above, and two of these tests would pass by id coincidence.
  beforeEach(() => {
    _resetForTests();
  });

  // A restored workspace layout keeps its remembered tab unless the selection was
  // an explicit user choice, so every entry point must classify itself correctly.
  it("marks a created session explicit", () => {
    createSession(makeSession({ id: "s1" }));
    expect(isSelectionExplicit("s1")).toBe(true);
  });

  it("marks a jump-to-index explicit", () => {
    createSession(makeSession({ id: "s1" }));
    createSession(makeSession({ id: "s2" }));
    jumpToSession(0);
    expect(isSelectionExplicit("s1")).toBe(true);
  });

  it("treats a plain selection as an arbitrary entry point", () => {
    createSession(makeSession({ id: "s1" }));
    selectSession("s1");
    expect(isSelectionExplicit("s1")).toBe(false);
  });

  it("clears the mark when the selection moves to another session", () => {
    createSession(makeSession({ id: "s1" }));
    createSession(makeSession({ id: "s2" }));
    selectSession("s1");
    expect(isSelectionExplicit("s2")).toBe(false);
    expect(isSelectionExplicit("s1")).toBe(false);
  });

  it("does not leak an explicit mark onto a fallback selection", async () => {
    // Deleting the explicitly chosen session falls back to sessions[0]; treating
    // that as a choice would override the remembered tab on the next load.
    createSession(makeSession({ id: "s1" }));
    const second = makeSession({ id: "s2" });
    createSession(second);
    expect(isSelectionExplicit("s2")).toBe(true);

    await deleteSession(second);

    const active = getActiveSessionId();
    expect(active).toBe("s1");
    expect(isSelectionExplicit(active!)).toBe(false);
  });
});
