# ADR-0006: Config file as source of truth for settings

## Status

Accepted

## Context

Settings (appearance, terminal preferences) were stored in SQLite. We need to add provider definitions (multi-agent support) and want users to be able to edit configuration in their editor, version-control it, or sync it across machines. Options considered:

1. **Keep SQLite** — add provider columns to the settings table. Simple but not portable or user-editable.
2. **Config file as source of truth** — a JSON file the app reads on launch and the settings page writes back to. SQLite settings table is eliminated.
3. **Hybrid** — config file seeds the DB, DB is runtime truth. Two systems to maintain, confusing conflict semantics.

## Decision

Use `$XDG_CONFIG_HOME/planeai/config.json` (default `~/.config/planeai/config.json`) as the single source of truth for all user preferences and provider definitions.

Key details:

- **JSONC for reading** (comments allowed), pretty JSON for writing.
- **`config.schema.json`** written alongside the config for editor autocomplete.
- **Read on launch only** — no file watcher (deferred).
- **Lenient validation** — malformed config merges with defaults; UI shows a toast on issues.
- **First launch** — if no config exists, create one with defaults (Kiro provider preset). If SQLite settings exist, migrate them into the new config file (one-time).
- **Settings page** is a GUI editor for the file — reads from it, writes back to it.

## Consequences

- Users can edit config in any editor, commit it to dotfiles, symlink across machines.
- SQLite `settings` table is deprecated and eventually removed.
- One-time migration needed for existing users.
- No live reload — external edits require app restart (acceptable for now).
- Provider definitions live in the config, enabling multi-agent support without DB schema changes.
