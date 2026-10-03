# ADR-0013: Category settings window driven by a settings registry

## Status

Accepted

## Context

Preferences was one 1128-line component with six top tabs.
A "More" tab had become a catch-all holding unrelated settings: the session backend, Vim mode, updates, the CLI and logs.
Users could not find settings, and several config keys (`scrollback_lines`, `web_links`, `extra_path_dirs`) had no UI at all.
Agents were edited as raw provider entries, with presets offered only as "add" buttons, and nothing showed whether an agent's binary existed.
There was no way to see that a value differed from its default, or to link to a specific setting.

Variants were prototyped with throwaway UI, then compared side by side: inset cards (macOS System Settings style), one long scrolling page (VS Code style), and a section split (Linear/GitHub style).
The chosen design merges the inset cards with the section split's help text, per-setting reset and jump-to-setting search.

## Decision

- Preferences stays a separate window, now 920x680 by default (min 760x520), and remembers its size.
- A left sidebar lists workflow categories: General, Appearance, Terminal, Agents, Sessions, Tasks, Editor, Plugins and Advanced.
  Each enabled plugin's `preferences` contribution gets its own sidebar entry under Plugins.
  There is no catch-all category.
- Every user-facing setting has one entry in `src/lib/settings-registry.ts`: id, category, section, label, description and keywords.
  The registry is the single source for row labels, search, deep links and reset, and page components render `SettingRow`s by registry id.
- Search is inline in the sidebar (⌘F / Ctrl+F).
  Results replace the page, show a breadcrumb for each setting, and selecting one (or pressing Enter for the first) jumps to the setting and briefly highlights it.
  Plugin pages are matched by plugin and contribution name only; settings inside a plugin page are not indexed.
- Scalar settings offer Reset when their effective value differs from the backend's `Config::default()`, which `get_config_defaults` exposes.
  Defaults are never duplicated in TypeScript.
  An optional field resets by being cleared.
  List-shaped settings (agents, language-server profiles) have no reset.
- Deep links take the form `?page=preferences&section=<category>#<setting>` or `?page=preferences&plugin=<pluginId>:<contributionId>`.
  An already-open window receives the target through the `preferences-navigate` event.
  A plugin asking to open preferences lands on its own page.
- An agent is enabled exactly when it is present in `providers`, with no separate flag.
  The four presets are always listed: turning one on writes the preset entry, and turning it off removes the entry.
  The default agent cannot be turned off, which also prevents removing the last agent.
  Custom agents are deleted instead of toggled.
  Each agent shows the binary that `detect_providers` resolved against the session PATH.
- Settings apply instantly.
  Two exceptions: the file editor command is validated as a whole and keeps an explicit Save, and dialogs edit agents and language-server profiles.

## Consequences

- Adding a setting means adding a registry entry and a `SettingRow` on its category page.
  A setting missing from the registry renders its raw id as the label, so the omission is visible.
- Search quality depends on keywords in the registry, not on page markup.
- A frontend-only config field is silently dropped by `update_config`.
  `post_merge_action` was affected and has been added to the Rust `Config`.
  New settings must exist on both sides.
