import { describe, it, expect } from "vitest";
import {
  isActive,
  isTerminal,
  isInterventionRequired,
  canTick,
  canStop,
  canStart,
  statusColor,
  statusBadgeColor,
  statusLabel,
  type LoopStatusValue,
} from "../loop-status";

const ALL_STATUSES: LoopStatusValue[] = [
  "draft",
  "running",
  "observing",
  "verifying",
  "completed_unreviewed",
  "blocked",
  "needs_human",
  "stale",
  "failed",
  "cancelled",
  "approved",
  "merged",
  "cleaned",
];

describe("loop-status", () => {
  describe("isActive", () => {
    it("returns true for running, observing, verifying", () => {
      expect(isActive("running")).toBe(true);
      expect(isActive("observing")).toBe(true);
      expect(isActive("verifying")).toBe(true);
    });

    it("returns false for all other statuses", () => {
      const inactive = ALL_STATUSES.filter(
        (s) => !["running", "observing", "verifying"].includes(s),
      );
      for (const s of inactive) {
        expect(isActive(s)).toBe(false);
      }
    });
  });

  describe("isTerminal", () => {
    it("returns true for terminal statuses", () => {
      expect(isTerminal("failed")).toBe(true);
      expect(isTerminal("cancelled")).toBe(true);
      expect(isTerminal("approved")).toBe(true);
      expect(isTerminal("merged")).toBe(true);
      expect(isTerminal("cleaned")).toBe(true);
    });

    it("returns false for non-terminal statuses", () => {
      const nonTerminal = ALL_STATUSES.filter(
        (s) =>
          !["failed", "cancelled", "approved", "merged", "cleaned"].includes(s),
      );
      for (const s of nonTerminal) {
        expect(isTerminal(s)).toBe(false);
      }
    });
  });

  describe("isInterventionRequired", () => {
    it("returns true for statuses needing human attention", () => {
      expect(isInterventionRequired("blocked")).toBe(true);
      expect(isInterventionRequired("needs_human")).toBe(true);
      expect(isInterventionRequired("completed_unreviewed")).toBe(true);
      expect(isInterventionRequired("stale")).toBe(true);
    });

    it("returns false for statuses not needing intervention", () => {
      const noIntervention = ALL_STATUSES.filter(
        (s) =>
          !["blocked", "needs_human", "completed_unreviewed", "stale"].includes(
            s,
          ),
      );
      for (const s of noIntervention) {
        expect(isInterventionRequired(s)).toBe(false);
      }
    });
  });

  describe("canTick", () => {
    it("allows tick only for active statuses", () => {
      expect(canTick("running")).toBe(true);
      expect(canTick("observing")).toBe(true);
      expect(canTick("verifying")).toBe(true);
    });

    it("disallows tick for draft", () => {
      expect(canTick("draft")).toBe(false);
    });

    it("disallows tick for terminal statuses", () => {
      expect(canTick("failed")).toBe(false);
      expect(canTick("cancelled")).toBe(false);
      expect(canTick("merged")).toBe(false);
    });

    it("disallows tick for intervention-required statuses", () => {
      expect(canTick("blocked")).toBe(false);
      expect(canTick("needs_human")).toBe(false);
      expect(canTick("completed_unreviewed")).toBe(false);
      expect(canTick("stale")).toBe(false);
    });
  });

  describe("canStop", () => {
    it("allows stop only for active statuses", () => {
      expect(canStop("running")).toBe(true);
      expect(canStop("observing")).toBe(true);
      expect(canStop("verifying")).toBe(true);
    });

    it("disallows stop for non-active statuses", () => {
      expect(canStop("draft")).toBe(false);
      expect(canStop("failed")).toBe(false);
      expect(canStop("cancelled")).toBe(false);
      expect(canStop("blocked")).toBe(false);
    });
  });

  describe("canStart", () => {
    it("allows start only for draft", () => {
      expect(canStart("draft")).toBe(true);
    });

    it("disallows start for all other statuses", () => {
      const nonDraft = ALL_STATUSES.filter((s) => s !== "draft");
      for (const s of nonDraft) {
        expect(canStart(s)).toBe(false);
      }
    });
  });

  describe("statusColor", () => {
    it("returns a class string for every status", () => {
      for (const s of ALL_STATUSES) {
        const result = statusColor(s);
        expect(result).toMatch(/^bg-/);
      }
    });

    it("returns expected colors for key statuses", () => {
      expect(statusColor("draft")).toBe("bg-t3");
      expect(statusColor("running")).toBe("bg-status-running");
      expect(statusColor("failed")).toBe("bg-status-exited");
      expect(statusColor("merged")).toBe("bg-status-idle");
    });
  });

  describe("statusBadgeColor", () => {
    it("returns a compound class string for every status", () => {
      for (const s of ALL_STATUSES) {
        const result = statusBadgeColor(s);
        expect(result).toContain("bg-");
        expect(result).toContain("text-");
      }
    });

    it("returns expected badge colors for key statuses", () => {
      expect(statusBadgeColor("running")).toBe(
        "bg-status-running/20 text-status-running",
      );
      expect(statusBadgeColor("failed")).toBe(
        "bg-status-exited/20 text-status-exited",
      );
    });
  });

  describe("statusLabel", () => {
    it("returns a non-empty label for every status", () => {
      for (const s of ALL_STATUSES) {
        expect(statusLabel(s).length).toBeGreaterThan(0);
      }
    });

    it("returns expected labels", () => {
      expect(statusLabel("draft")).toBe("Draft");
      expect(statusLabel("completed_unreviewed")).toBe("Needs Review");
      expect(statusLabel("needs_human")).toBe("Needs Human");
    });
  });

  describe("partition coverage", () => {
    it("every status is in exactly one of: active, terminal, interventionRequired, or draft", () => {
      for (const s of ALL_STATUSES) {
        const categories = [
          isActive(s),
          isTerminal(s),
          isInterventionRequired(s),
          s === "draft",
        ].filter(Boolean);
        expect(categories.length).toBe(1);
      }
    });
  });
});
