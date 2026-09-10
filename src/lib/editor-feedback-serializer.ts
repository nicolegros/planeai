import type { EditorFeedback } from "./editor-feedback.svelte";

export function serializeEditorFeedback(feedback: readonly EditorFeedback[]): string {
  if (feedback.length === 0) return "";

  const lines = ["Please address this editor feedback:"];
  for (const item of feedback) {
    const lineLabel =
      item.startLine === item.endLine
        ? `line ${item.startLine}`
        : `lines ${item.startLine}-${item.endLine}`;
    const language = item.language;

    lines.push("");
    lines.push(
      `--- ${item.filePath} (${lineLabel}${item.isUnsaved ? "; unsaved editor content" : ""}) ---`,
    );
    appendCodeBlock(lines, "Context before selection:", item.contextBefore, language);
    appendCodeBlock(lines, "Selected code:", item.selectedText, language);
    appendCodeBlock(lines, "Context after selection:", item.contextAfter, language);
    lines.push(`Comment: ${item.text}`);
  }
  lines.push("");
  return lines.join("\n");
}

function appendCodeBlock(lines: string[], label: string, content: string, language: string): void {
  if (!content) return;
  const longestBacktickRun = Math.max(
    2,
    ...Array.from(content.matchAll(/`+/g), (match) => match[0].length),
  );
  const fence = "`".repeat(longestBacktickRun + 1);
  lines.push(label);
  lines.push(`${fence}${language}`);
  lines.push(content);
  lines.push(fence);
}
