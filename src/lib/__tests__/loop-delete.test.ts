import { describe, it, expect } from "vitest";
import type { Session, LoopRunSummary, LoopSessionItem } from "../types";
import type { LoopStatusValue } from "../loop-status";
import { isActive } from "../loop-status";

/**
 * Tests for PLA-239: loop delete via keyboard (dd) and right-click context menu.
 *
 * Verifies the logic that determines:
 * - Whether a loop delete shows a confirmation dialog (has sessions) or instant-deletes
 * - Whether deleting a loop-linked session is refused (active loop) or allowed (stopped loop)
 * - Context menu items for loop sessions
 */

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    project_id: "p1",
    name: "my-session",
    tmux_name: null,
    branch: "feat-1",
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "direct",
    tab_count: 1,
    base_branch: null,
    task_key: null,
    pr_url: null,
    pr_state: null,
    ...overrides,
  };
}

function makeLoop(overrides: Partial<LoopRunSummary> = {}): LoopRunSummary {
  return {
    id: "loop-1",
    project_id: "p1",
    task_key: "PLA-100",
    strategy: "maker-verifier",
    goal: "Implement feature X",
    status: "running",
    max_rounds: 5,
    created_at: "",
    updated_at: "",
    ...overrides,
  };
}

function makeLoopSessionItem(overrides: Partial<LoopSessionItem> = {}): LoopSessionItem {
  return {
    session_id: "s1",
    role: "maker",
    round: 1,
    provider: "claude",
    status: "active",
    created_at: "",
    ...overrides,
  };
}

// ─── Decision logic replicated from App.svelte ────────────────────────────────

/**
 * Determines whether deleting a loop should show a confirmation dialog.
 * Returns true if dialog is needed (loop has sessions), false for instant delete.
 */
function shouldShowLoopDeleteDialog(loopSessions: LoopSessionItem[]): boolean {
  return loopSessions.length > 0;
}

/**
 * Determines whether deleting a loop-linked session should be refused.
 * Returns an error message if refused, null if allowed.
 */
function canDeleteLoopSession(loopStatus: LoopStatusValue): string | null {
  if (isActive(loopStatus)) {
    return "Stop the loop before deleting its sessions";
  }
  return null;
}

// ─── Context menu construction replicated from UnifiedSidebar.svelte ──────────

type MenuItem =
  | { label: string; danger?: boolean; onSelect: () => void }
  | { label: string; children: MenuItem[] };

function buildLoopContextMenu(loop: LoopRunSummary): MenuItem[] {
  return [
    ...(loop.status === "draft" ? [{ label: "Start loop", onSelect: () => {} }] : []),
    ...(isActive(loop.status) ? [{ label: "Stop loop", onSelect: () => {} }] : []),
    { label: "Delete loop", danger: true, onSelect: () => {} },
  ];
}

function buildLoopSessionContextMenu(_session: Session): MenuItem[] {
  return [
    { label: "Review", onSelect: () => {} },
    { label: "Delete", danger: true, onSelect: () => {} },
  ];
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("loop delete (PLA-239)", () => {
  describe("loop delete confirmation dialog logic", () => {
    it("shows dialog when loop has sessions", () => {
      const sessions = [makeLoopSessionItem()];
      expect(shouldShowLoopDeleteDialog(sessions)).toBe(true);
    });

    it("does not show dialog when loop has no sessions", () => {
      expect(shouldShowLoopDeleteDialog([])).toBe(false);
    });

    it("shows dialog even when loop has only one session", () => {
      const sessions = [makeLoopSessionItem({ session_id: "s1" })];
      expect(shouldShowLoopDeleteDialog(sessions)).toBe(true);
    });
  });

  describe("loop session delete guard", () => {
    it("refuses delete when loop is running", () => {
      expect(canDeleteLoopSession("running")).toBe("Stop the loop before deleting its sessions");
    });

    it("refuses delete when loop is observing", () => {
      expect(canDeleteLoopSession("observing")).toBe("Stop the loop before deleting its sessions");
    });

    it("refuses delete when loop is verifying", () => {
      expect(canDeleteLoopSession("verifying")).toBe("Stop the loop before deleting its sessions");
    });

    it("allows delete when loop is completed_unreviewed", () => {
      expect(canDeleteLoopSession("completed_unreviewed")).toBeNull();
    });

    it("allows delete when loop is failed", () => {
      expect(canDeleteLoopSession("failed")).toBeNull();
    });

    it("allows delete when loop is cancelled", () => {
      expect(canDeleteLoopSession("cancelled")).toBeNull();
    });

    it("allows delete when loop is approved", () => {
      expect(canDeleteLoopSession("approved")).toBeNull();
    });

    it("allows delete when loop is merged", () => {
      expect(canDeleteLoopSession("merged")).toBeNull();
    });

    it("allows delete when loop is cleaned", () => {
      expect(canDeleteLoopSession("cleaned")).toBeNull();
    });

    it("allows delete when loop is draft", () => {
      expect(canDeleteLoopSession("draft")).toBeNull();
    });

    it("allows delete when loop is blocked", () => {
      expect(canDeleteLoopSession("blocked")).toBeNull();
    });

    it("allows delete when loop is needs_human", () => {
      expect(canDeleteLoopSession("needs_human")).toBeNull();
    });

    it("allows delete when loop is stale", () => {
      expect(canDeleteLoopSession("stale")).toBeNull();
    });
  });

  describe("loop context menu", () => {
    it("includes 'Delete loop' for running loop", () => {
      const loop = makeLoop({ status: "running" });
      const items = buildLoopContextMenu(loop);
      const labels = items.map((i) => i.label);
      expect(labels).toContain("Delete loop");
    });

    it("marks 'Delete loop' as danger", () => {
      const loop = makeLoop({ status: "running" });
      const items = buildLoopContextMenu(loop);
      const del = items.find((i) => i.label === "Delete loop");
      expect(del && "danger" in del && del.danger).toBe(true);
    });

    it("includes 'Stop loop' for active loop", () => {
      const loop = makeLoop({ status: "running" });
      const items = buildLoopContextMenu(loop);
      expect(items.find((i) => i.label === "Stop loop")).toBeDefined();
    });

    it("includes 'Start loop' for draft loop", () => {
      const loop = makeLoop({ status: "draft" });
      const items = buildLoopContextMenu(loop);
      expect(items.find((i) => i.label === "Start loop")).toBeDefined();
    });

    it("does not include 'Start loop' for non-draft loop", () => {
      const loop = makeLoop({ status: "failed" });
      const items = buildLoopContextMenu(loop);
      expect(items.find((i) => i.label === "Start loop")).toBeUndefined();
    });

    it("does not include 'Stop loop' for non-active loop", () => {
      const loop = makeLoop({ status: "failed" });
      const items = buildLoopContextMenu(loop);
      expect(items.find((i) => i.label === "Stop loop")).toBeUndefined();
    });
  });

  describe("loop session context menu", () => {
    it("includes Review and Delete items", () => {
      const session = makeSession();
      const items = buildLoopSessionContextMenu(session);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual(["Review", "Delete"]);
    });

    it("marks Delete as danger", () => {
      const session = makeSession();
      const items = buildLoopSessionContextMenu(session);
      const del = items.find((i) => i.label === "Delete");
      expect(del && "danger" in del && del.danger).toBe(true);
    });
  });
});
