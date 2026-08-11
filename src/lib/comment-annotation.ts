import type { ReviewComment } from "./review-comments.svelte";

export interface CommentAnnotationHandlers {
  onOpen: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onContextMenu: (event: MouseEvent) => void;
}

/** Render a review annotation with separately operable open, edit, and delete controls. */
export function renderCommentAnnotation(
  comment: ReviewComment,
  handlers: CommentAnnotationHandlers,
): HTMLDivElement {
  const location =
    comment.startLine === comment.endLine
      ? `line ${comment.startLine}`
      : `lines ${comment.startLine}–${comment.endLine}`;
  const el = document.createElement("div");
  el.style.cssText =
    "padding:6px 10px;margin:2px 0;border-radius:4px;font-size:12px;line-height:1.4;display:flex;align-items:flex-start;gap:8px;background:var(--comment-bg,rgba(128,128,128,0.1));border:1px solid var(--comment-border,rgba(128,128,128,0.2))";
  el.dataset.reviewCommentId = comment.id;
  el.setAttribute("role", "group");
  el.setAttribute("aria-label", `Review comment on ${location}`);
  el.oncontextmenu = handlers.onContextMenu;

  const open = document.createElement("button");
  open.type = "button";
  open.style.cssText =
    "flex:1;background:none;border:none;padding:0;text-align:left;white-space:pre-wrap;word-break:break-word;cursor:pointer";
  open.textContent = comment.text;
  open.title = `Open comment on ${location}`;
  open.onclick = (event) => {
    event.stopPropagation();
    handlers.onOpen();
  };

  const edit = document.createElement("button");
  edit.type = "button";
  edit.style.cssText =
    "background:none;border:none;cursor:pointer;padding:2px;color:#888;font-size:12px";
  edit.textContent = "✎";
  edit.title = `Edit comment on ${location}`;
  edit.setAttribute("aria-label", `Edit comment on ${location}`);
  edit.onclick = (event) => {
    event.stopPropagation();
    handlers.onEdit();
  };

  const del = document.createElement("button");
  del.type = "button";
  del.style.cssText =
    "background:none;border:none;cursor:pointer;padding:2px;color:#888;font-size:14px";
  del.textContent = "×";
  del.title = `Delete comment on ${location}`;
  del.setAttribute("aria-label", `Delete comment on ${location}`);
  del.onclick = (event) => {
    event.stopPropagation();
    handlers.onDelete();
  };

  el.append(open, edit, del);
  return el;
}
