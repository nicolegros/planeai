# planeai

A cross-platform app for running multiple AI coding agents in parallel. Each agent works in its own terminal session, backed by tmux for persistence.

## Features (v1)

- **Keyboard-first** — every action reachable without a mouse
- **Session management** — launch Kiro agents, switch between sessions
- **Sidebar** — sessions grouped by project, active session highlighted
- **Tab switcher** — Ctrl+Tab MRU overlay for fast session switching
- **tmux persistence** — agents keep running when you quit the app
- **SQLite storage** — projects and sessions persisted locally

## Requirements

- macOS (Linux/Windows planned)
- tmux (`brew install tmux`)
- `kiro-cli` on PATH

## Tech stack

- **Tauri v2** — Rust backend, webview shell
- **Svelte 5** — reactive UI with runes
- **xterm.js** — terminal rendering in the browser
- **Tailwind CSS** — utility-first styling, dark theme
- **rusqlite** — SQLite persistence on the Rust side
- **pnpm** — package management

## Architecture

The Rust backend owns tmux interaction, SQLite, and PTY management. The Svelte frontend renders the UI and terminal via xterm.js. Communication is via Tauri IPC (commands for actions, event channels for streaming terminal bytes).

See [CONTEXT.md](./CONTEXT.md) for domain glossary and [docs/adr/](./docs/adr/) for architecture decisions.

## Development

This project uses TDD. See [AGENTS.md](./AGENTS.md) for workflow guidelines.

```bash
pnpm install
pnpm tauri dev
```
