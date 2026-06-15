# ADR-0009: Typed API layer for Tauri IPC

## Status

Accepted

## Context

The frontend had 46 raw `invoke("command_name", { ... })` calls scattered across 13+ files. Command names were stringly-typed — renaming a command in Rust silently broke the UI with no compile error. The `Session` TypeScript interface was duplicated in 7 files with inconsistent field subsets. No type generation tooling existed (no tauri-specta, ts-rs, or typeshare).

This made refactoring risky and slowed development velocity. We needed a typed seam before extracting the SessionOrchestrator from App.svelte.

## Decision

Consolidate all Tauri IPC calls behind two modules:

1. **`src/lib/types.ts`** — canonical TypeScript interfaces matching Rust structs (Session, Project, TaskItem, DirEntry, ChangedFile, FileDiff). Single source of truth; components import from here.

2. **`src/lib/api.ts`** — domain-grouped typed methods wrapping `invoke()`. Organized into namespaces: `sessions`, `projects`, `pty`, `config`, `tasks`, `fileExplorer`, `git`, `notify`, `symphony`, `preferences`.

Components never call `invoke()` directly. Tests mock the api module rather than `@tauri-apps/api/core`.

### Rejected alternatives

- **tauri-specta** (full codegen from Rust types) — good long-term but heavy setup. The manual typed layer is the immediate win and is forward-compatible with adding specta later.
- **Per-component service files** — fragments the seam. One API module is the right granularity for this codebase size.
- **Barrel re-exports only** — doesn't provide the domain grouping or parameter type safety.

## Consequences

- Command renames in Rust now produce a single compile error in `api.ts` rather than silent runtime failures across multiple components.
- Components are decoupled from the IPC mechanism — swapping to specta-generated bindings later requires changes only in `api.ts`.
- Tests are more focused — mocking `api.sessions.list` is clearer than matching on command name strings.
- New commands require adding a method to `api.ts` and optionally a type to `types.ts`.
- The `Channel` type from `@tauri-apps/api/core` is still imported directly in `Terminal.svelte` (it's a constructor, not an invoke call).
