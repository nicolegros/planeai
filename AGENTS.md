# Agents

## Commands

```bash
make ci              # Full local validation: lint + test (both stacks)
make lint            # Formatting + static analysis only
make test            # Frontend + backend tests
make fmt             # Auto-fix formatting (both stacks)
pnpm test            # Frontend tests only (vitest)
pnpm lint            # oxlint only
pnpm exec svelte-check  # TypeScript type checking for Svelte
cd src-tauri && cargo test --workspace  # Backend tests only
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings  # Rust lints
```

Environment variables for Rust commands:

```bash
JIRA_CLIENT_ID=dummy JIRA_CLIENT_SECRET=dummy cargo test --workspace
```

The Makefile passes dummy values automatically — prefer `make test` over raw cargo commands.

## Project structure

- **`src/`** — Svelte 5 frontend (components, reactive stores, typed API layer)
- **`src-tauri/`** — Rust backend (Tauri v2 app, CLI, daemon, workspace of library crates)
- **`docs/`** — Documentation site (Astro) and architecture decision records
- **`CONTEXT.md`** — Domain glossary, architecture notes, key constraints

## Code style

### TypeScript / Svelte

- **Linter:** oxlint (not eslint)
- **Formatter:** oxfmt (not prettier)
- **Type checker:** svelte-check
- **Framework:** Svelte 5 with runes (`$state`, `$derived`, `$effect`) — no legacy stores, no `$:`
- **File naming:** kebab-case (`loop-store.svelte.ts`, `SessionForm.svelte`)
- **Reactive stores:** `.svelte.ts` extension (e.g., `task-store.svelte.ts`)
- **Components:** PascalCase filenames (`LoopDashboard.svelte`)
- **Complex interactives:** Use bits-ui (not custom popovers/selects)
- **Design tokens:** CSS custom properties via `@theme` block — never hardcode colors
- **API calls:** Use `src/lib/api.ts` typed layer — never call `invoke()` directly (ADR-0009)
- **Imports:** Prefer relative imports within `src/`

### Rust

- **Formatter:** `cargo fmt` (default rustfmt config)
- **Linter:** `cargo clippy -- -D warnings` (deny all warnings)
- **Error handling:** `anyhow::Result` for Tauri commands; typed errors for library crates
- **File naming:** snake_case
- **Async:** All Tauri commands must be `async`
- **Module naming:** One file per concern; use `mod.rs` only for multi-file modules

## Performance

Never block the main thread. On macOS, WKWebView dispatches all Tauri IPC handlers on the main thread — any synchronous work there stalls PTY data delivery and keystroke processing for every session.

- All Tauri commands that perform I/O must be `async`
- Use `commands::blocking(|| { ... }).await` for commands wrapping synchronous I/O
- Use `tokio::process::Command` for subprocess work (never `std::process::Command` in commands)
- Release Mutex locks before `.await`
- On Windows: apply `planeai_core::command::no_window()` or `no_window_tokio()` to all subprocess spawns

## Test patterns

### Frontend (vitest + jsdom)

- Tests live in `__tests__/` directories adjacent to source
- Use `describe` / `it` / `expect` pattern
- Mock Tauri `invoke` via `vi.mock("@tauri-apps/api/core")`
- Component tests mount with Svelte's testing APIs
- Run: `pnpm test`

### Backend (cargo test)

- Unit tests: inline `#[cfg(test)] mod tests` within the source file
- Integration tests: dedicated test files (e.g., `axi/tests.rs`, `config_tests.rs`)
- Use `tempfile::TempDir` for filesystem isolation
- Use `planeai-core` with `features = ["test-support"]` for test helpers
- Run: `cd src-tauri && cargo test --workspace`

### TDD workflow

Use red-green-refactor when implementing features and fixing bugs:

1. Write a failing test that captures the expected behavior
2. Write minimal code to make it pass
3. Refactor while keeping tests green

## Commits

Use conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
Scopes are optional but preferred: `feat(loops):`, `fix(daemon):`, `refactor(sidebar):`.

## Documentation

Keep documentation up to date with code changes. When a feature is added or modified, update the relevant docs (README.md, CONTEXT.md, ADRs) in the same commit.

