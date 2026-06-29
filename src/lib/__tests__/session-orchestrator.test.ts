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
    restart: vi.fn(() => Promise.resolve()),
    markExited: vi.fn(),
    acknowledge: vi.fn(() => Promise.resolve()),
    saveMruOrder: vi.fn(() => Promise.resolve()),
  },
  pr: {
    getCiChecks: vi.fn(() => Promise.resolve([])),
    getPrComments: vi.fn(() => Promise.resolve(0)),
  },
  pty: { closeTab: vi.fn(() => Promise.resolve()) },
  symphony: { getStatus: vi.fn(() => Promise.resolve("null")) },
  tasks: { fireNotifyHook: vi.fn(() => Promise.resolve()) },
  git: { getChangedFiles: vi.fn(() => Promise.resolve([])) },
}));

import { sessions as sessionsApi, symphony } from "../api";
import { getSettings } from "../settings.svelte";
import type { Session } from "../types";
import {
  getSessions,
  getActiveSessionId,
  loadSessions,
  selectSession,
  createSession,
  deleteSession,
  archiveSession,
  restartSession,
  getUnifiedTabs,
  getUnifiedActiveIndex,
  selectUnifiedTab,
  handleNextTab,
  handlePrevTab,
  toggleDiff,
  toggleEditor,
  getDiffTabOpen,
  getDiffTabActive,
  getEditorTabOpen,
  getEditorTabActive,
  getAgentStates,
  clearAgentState,
  startEventListeners,
  startSymphonyPolling,
  _resetForTests,
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
    pr_url: null,
    pr_state: null,
    ...overrides,
  };
}

describe("session-orchestrator", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    _resetForTests();
    api.list.mockResolvedValue([]);
  });

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
  });

  describe("restartSession", () => {
    it("replaces session with updated version", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1", status: "exited" })]);
      await loadSessions();
      api.restart.mockResolvedValue(makeSession({ id: "s1", status: "active" }));
      await restartSession(makeSession({ id: "s1", status: "exited" }));
      expect(getSessions().find((s) => s.id === "s1")?.status).toBe("active");
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

    it("toggleDiff opens and activates", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      toggleDiff();
      expect(getDiffTabOpen()["s1"]).toBe(true);
      expect(getDiffTabActive()["s1"]).toBe(true);
    });

    it("toggleDiff closes when active", async () => {
      api.list.mockResolvedValue([makeSession({ id: "s1" })]);
      await loadSessions();
      toggleDiff();
      toggleDiff();
      expect(getDiffTabOpen()["s1"]).toBe(false);
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
});
