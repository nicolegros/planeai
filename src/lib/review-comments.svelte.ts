import type { DiffSide } from "./review-diff";

export interface ReviewComment {
  id: string;
  filePath: string;
  type: "line" | "hunk" | "file";
  startLine: number;
  endLine: number;
  side: DiffSide;
  comparisonKey: string;
  fingerprint: string;
  text: string;
  createdAt: number;
}

type NewReviewComment = Omit<
  ReviewComment,
  "id" | "createdAt" | "side" | "comparisonKey" | "fingerprint"
> &
  Partial<Pick<ReviewComment, "side" | "comparisonKey" | "fingerprint">>;

let commentsBySession = $state<Record<string, ReviewComment[]>>({});

export function addComment(sessionId: string, comment: NewReviewComment): ReviewComment {
  const full: ReviewComment = {
    ...comment,
    side: comment.side ?? "modified",
    comparisonKey: comment.comparisonKey ?? "",
    fingerprint: comment.fingerprint ?? "",
    id: crypto.randomUUID(),
    createdAt: Date.now(),
  };
  commentsBySession[sessionId] = [...(commentsBySession[sessionId] ?? []), full];
  return full;
}

export function removeComment(sessionId: string, commentId: string): void {
  const list = commentsBySession[sessionId];
  if (list) commentsBySession[sessionId] = list.filter((c) => c.id !== commentId);
}

export function editComment(sessionId: string, commentId: string, newText: string): void {
  const list = commentsBySession[sessionId];
  if (!list) return;
  commentsBySession[sessionId] = list.map((c) =>
    c.id === commentId ? { ...c, text: newText } : c,
  );
}

/** Records that a file's pending review comments now refer to a refreshed diff. */
export function reanchorComments(
  sessionId: string,
  filePath: string,
  comparisonKey: string,
  fingerprint: string,
): void {
  const list = commentsBySession[sessionId];
  if (!list) return;
  commentsBySession[sessionId] = list.map((comment) =>
    comment.filePath === filePath ? { ...comment, comparisonKey, fingerprint } : comment,
  );
}

export function getComments(sessionId: string): ReviewComment[] {
  return commentsBySession[sessionId] ?? [];
}

export function getFileCommentCount(sessionId: string, filePath: string): number {
  return (commentsBySession[sessionId] ?? []).filter((c) => c.filePath === filePath).length;
}

export function getTotalCommentCount(sessionId: string): number {
  return (commentsBySession[sessionId] ?? []).length;
}

export function clearComments(sessionId: string): void {
  delete commentsBySession[sessionId];
}

export function _resetForTests(): void {
  commentsBySession = {};
}
