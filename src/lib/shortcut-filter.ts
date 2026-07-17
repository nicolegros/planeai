export interface ShortcutItem {
  keys: string;
  description: string;
}

export interface ShortcutSection {
  section: string;
  items: ShortcutItem[];
}

/**
 * Filter shortcut sections by a search query.
 * When query is empty, returns sections as-is (grouped).
 * When query is non-empty, returns a single flat section with all matching items.
 * Match is case-insensitive substring on the description field.
 */
export function filterShortcuts(sections: ShortcutSection[], query: string): ShortcutSection[] {
  const trimmed = query.trim();
  if (!trimmed) return sections;

  const lower = trimmed.toLowerCase();
  const matched: ShortcutItem[] = [];

  for (const section of sections) {
    for (const item of section.items) {
      if (item.description.toLowerCase().includes(lower)) {
        matched.push(item);
      }
    }
  }

  if (matched.length === 0) return [];

  return [{ section: "", items: matched }];
}
