# Diff Review Mouse Smoke Checklist

Run this in a development build with a session that has a multi-file diff, several hunks, and at least one collapsed unchanged-context separator.

1. Open **Review**, select a changed line, then drag across multiple lines. Verify the selected range is highlighted in both **Split** and **Unified** views. Repeat after switching files, refreshing (`r`), and toggling the view mode.
2. In split view, start a drag on either pane and cross the center gutter. Verify the visual selection remains on the pane where the drag began. Drag across a collapsed context separator and verify the range stops at that separator.
3. Confirm the `+` gutter control appears at the selection endpoint. Click it, enter a comment, submit it, and verify the comment is attached to the selected line/range. Select a deleted-side line and verify it follows the same existing comment behavior as keyboard selection.
4. Right-click a line/range and verify the app context menu offers **Comment on line(s)**; where an annotation overlaps, verify it also offers **Edit comment**. Verify a right-click outside code clears the selection.
5. Click a collapsed context separator to expand that block. Right-click it and verify **Expand context block** and **Expand all context in file** both work.
6. Click an existing inline annotation and a split-view comment-list entry. Each should open its editor. Right-click either and verify the custom menu offers **Edit comment** and **Delete comment**.
7. Verify the persistent toolbar hint says `⌥ drag: select text`. Hold Option while dragging partial code text: native text selection and copy must work, and review-line selection must not start.
8. Start a non-empty comment draft. Select another range in the same file and verify the draft remains intact. Then try switching files, refreshing, changing split/unified mode, and changing the comparison: each must request confirmation before discarding the draft. Cancel leaves the draft; confirm discards it.
9. Verify selections clear after submitting or canceling a comment, pressing Escape, selecting a different file, clicking a different line, or clicking blank diff space.
