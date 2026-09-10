export interface EditorFeedbackSnapshot {
  filePath: string;
  startLine: number;
  endLine: number;
  language: string;
  selectedText: string;
  contextBefore: string;
  contextAfter: string;
  isUnsaved: boolean;
}

export interface EditorFeedback extends EditorFeedbackSnapshot {
  id: string;
  text: string;
  createdAt: number;
}

type NewEditorFeedback = EditorFeedbackSnapshot & Pick<EditorFeedback, "text">;

let feedbackBySession = $state<Record<string, EditorFeedback[]>>({});

export function addEditorFeedback(sessionId: string, feedback: NewEditorFeedback): EditorFeedback {
  const full: EditorFeedback = {
    ...feedback,
    id: crypto.randomUUID(),
    createdAt: Date.now(),
  };
  feedbackBySession[sessionId] = [...(feedbackBySession[sessionId] ?? []), full];
  return full;
}

export function getEditorFeedback(sessionId: string): readonly EditorFeedback[] {
  return feedbackBySession[sessionId] ?? [];
}

export function getEditorFeedbackCount(sessionId: string): number {
  return (feedbackBySession[sessionId] ?? []).length;
}

export function editEditorFeedback(sessionId: string, feedbackId: string, text: string): void {
  const feedback = feedbackBySession[sessionId];
  if (!feedback) return;
  feedbackBySession[sessionId] = feedback.map((item) =>
    item.id === feedbackId ? { ...item, text } : item,
  );
}

export function removeEditorFeedback(sessionId: string, feedbackId: string): void {
  const feedback = feedbackBySession[sessionId];
  if (feedback) feedbackBySession[sessionId] = feedback.filter((item) => item.id !== feedbackId);
}

export function clearEditorFeedback(sessionId: string): void {
  delete feedbackBySession[sessionId];
}

export function _resetForTests(): void {
  feedbackBySession = {};
}
