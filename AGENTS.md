# Agents

## Development workflow

Use TDD (red-green-refactor) when implementing features and fixing bugs. Write a failing test first, make it pass with minimal code, then refactor.

## Performance

Never block the main thread. All Tauri commands that perform I/O (subprocesses, network, disk) must be `async` using `tokio`. Use `tokio::process::Command` instead of `std::process::Command`. Release Mutex locks before awaiting.

## Commits

Use conventional commits (e.g., `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).

## Documentation

Keep documentation up to date with code changes. When a feature is added or modified, update the relevant docs (README.md, CONTEXT.md, ADRs) in the same commit.
