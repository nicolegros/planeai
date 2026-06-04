# planeai

A desktop app that lets you run multiple AI coding agents in parallel — each in its own persistent terminal session, orchestrated from a keyboard-first UI. Works with Kiro, Claude, Copilot, or any CLI-based agent.

> **Status:** Early development. Expect breaking changes between releases.

<!-- TODO: Add screenshot/GIF showing the main UI with multiple sessions running -->

## Features

- **Parallel agents** — run as many AI coding sessions as you need, side by side
- **Persistent sessions** — agents keep running when you quit the app (tmux backend)
- **Provider-agnostic** — works with Kiro, Claude, Copilot, or any CLI agent
- **Keyboard-first** — command menu (Cmd+K), shortcuts for every action
- **Task manager integration** — auto-create sessions from Jira, kanban, or custom task CLIs
- **Git worktree isolation** — parallel agents work on separate branches without conflicts
- **Cross-platform** — macOS, Linux, and Windows

## Install

Download the latest release for your platform:

| Platform              | Format                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| macOS (Apple Silicon) | [`.dmg`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |
| Linux                 | [`.deb`](https://github.com/nicolegros/planeai/releases/latest) / [`.AppImage`](https://github.com/nicolegros/planeai/releases/latest) |
| Windows               | [`.exe`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |

### Requirements

- tmux (optional, for persistent sessions — `brew install tmux` on macOS)
- At least one AI agent CLI on PATH (e.g., `kiro-cli`, `claude`, `gh copilot`)

## Configuration

planeai is configured via `~/.config/planeai/config.json`. See the full [configuration docs](./docs/configuration.md) for providers, task managers, templates, and lifecycle hooks.

## Development

```bash
pnpm install
pnpm tauri dev
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for prerequisites, testing, and contribution guidelines.

## Tech Stack

- **Tauri v2** — Rust backend, webview shell
- **Svelte 5** — reactive UI with runes
- **xterm.js** — terminal rendering
- **Tailwind CSS** — utility-first styling
- **SQLite** — local persistence

## License

[MIT](./LICENSE)
