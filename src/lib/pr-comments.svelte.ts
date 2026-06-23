import { pr } from "./api";
import type { PrCommentInfo, Session } from "./types";

let prComments = $state<Record<string, PrCommentInfo>>({});
let activeSessions: Session[] = [];

export function getPrCommentInfo(sessionId: string): PrCommentInfo | null {
  return prComments[sessionId] ?? null;
}

export function getCommentCount(sessionId: string): number {
  return prComments[sessionId]?.comment_count ?? 0;
}

export function updateSessions(sessions: Session[]): void {
  activeSessions = sessions;
}

export async function pollPrComments(): Promise<void> {
  const targets = activeSessions.filter((s) => s.status === "active" && s.pr_url);
  if (targets.length === 0) return;
  const results = await Promise.allSettled(
    targets.map(async (s) => {
      const info = await pr.getPrComments(s.id);
      return { id: s.id, info };
    }),
  );
  const next: Record<string, PrCommentInfo> = { ...prComments };
  for (const r of results) {
    if (r.status === "fulfilled") {
      next[r.value.id] = r.value.info;
    }
  }
  prComments = next;
}
