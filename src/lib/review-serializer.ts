import type { ReviewComment } from "./review-comments.svelte";
import type { FileDiff } from "./types";

export function serializeComments(
  comments: ReviewComment[],
  fileDiffs: Map<string, FileDiff>,
): string {
  if (comments.length === 0) return "";

  const grouped = Map.groupBy(comments, (c) => c.filePath);
  const lines: string[] = ["Please address these review comments:"];

  for (const [filePath, fileComments] of grouped) {
    const sorted = fileComments!.sort((a, b) => a.startLine - b.startLine);
    const diff = fileDiffs.get(filePath);
    const modifiedLines = diff?.modified.split("\n") ?? [];
    const lang = diff?.language ?? "";

    for (const comment of sorted) {
      lines.push("");
      if (comment.type === "file") {
        lines.push(`--- ${filePath} (file-level) ---`);
        lines.push(`Comment: ${comment.text}`);
      } else {
        const lineLabel =
          comment.startLine === comment.endLine
            ? `line ${comment.startLine}`
            : `lines ${comment.startLine}-${comment.endLine}`;
        lines.push(`--- ${filePath} (${lineLabel}) ---`);

        const ctxStart = Math.max(0, comment.startLine - 1 - 2);
        const ctxEnd = Math.min(modifiedLines.length, comment.endLine + 2);
        const contextSlice = modifiedLines.slice(ctxStart, ctxEnd);

        if (contextSlice.length > 0) {
          lines.push("```" + lang);
          lines.push(...contextSlice);
          lines.push("```");
        }
        lines.push(`Comment: ${comment.text}`);
      }
    }
  }

  lines.push("");
  return lines.join("\n");
}
