import { describe, expect, it } from "vitest";
import { renderCommentAnnotation } from "../comment-annotation";

const comment = {
  id: "comment-1",
  filePath: "src/example.ts",
  type: "line" as const,
  startLine: 7,
  endLine: 7,
  text: "Please simplify this branch.",
  createdAt: 1,
};

describe("renderCommentAnnotation", () => {
  it("keeps open, edit, and delete independently keyboard-operable", () => {
    const actions: string[] = [];
    const annotation = renderCommentAnnotation(comment, {
      onOpen: () => actions.push("open"),
      onEdit: () => actions.push("edit"),
      onDelete: () => actions.push("delete"),
      onContextMenu: () => actions.push("context-menu"),
    });
    document.body.append(annotation);

    expect(annotation.getAttribute("role")).toBe("group");
    expect(annotation.getAttribute("aria-label")).toBe("Review comment on line 7");
    const buttons = Array.from(annotation.querySelectorAll("button"));
    expect(buttons).toHaveLength(3);
    expect(buttons[0]?.textContent).toBe(comment.text);
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      null,
      "Edit comment on line 7",
      "Delete comment on line 7",
    ]);
    expect(buttons.every((button) => button.type === "button")).toBe(true);

    buttons.forEach((button) => button.click());
    annotation.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    expect(actions).toEqual(["open", "edit", "delete", "context-menu"]);

    annotation.remove();
  });
});
