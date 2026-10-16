---
title: Getting Started
description: Install planeai and run your first AI agent session.
draft: false
---

## Install

### macOS (Homebrew)

```bash
brew install --cask nicolegros/tap/planeai
```

Already installed planeai from the `.dmg`?
Let Homebrew take over the existing app instead:

```bash
brew install --cask --adopt nicolegros/tap/planeai
```

The cask also puts `planeai-cli` on your PATH.
The app keeps updating itself, so `brew upgrade` is not needed.
Homebrew only loads casks from taps you trust, so use the full name `nicolegros/tap/planeai` in other `brew` commands or run `brew trust nicolegros/tap` once.
`brew uninstall --zap --cask nicolegros/tap/planeai` also moves planeai's settings, data and `~/.planeai` worktrees to the Trash.

### Direct download

Download the latest release for your platform:

| Platform              | Format                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| macOS (Apple Silicon) | [`.dmg`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |
| macOS (Intel)         | [`.dmg`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |
| Linux                 | [`.deb`](https://github.com/nicolegros/planeai/releases/latest) / [`.AppImage`](https://github.com/nicolegros/planeai/releases/latest) |
| Windows               | [`.exe`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |

## Requirements

- **tmux** (optional, for persistent sessions) — `brew install tmux` on macOS
- **At least one AI agent CLI on PATH** — e.g., `kiro-cli`, `claude`, `codex`, `gh copilot`

## Basic Usage

Launch planeai after installation. Use **Cmd+K** (macOS) or **Ctrl+K** (Linux/Windows) to open the command menu and create your first session.

## Development

To run planeai from source:

```bash
pnpm install
make dev
```

## Next Steps

- [Configuration](/planeai/guides/configuration/) — set up providers, templates, and hooks
- [Theming](/planeai/guides/theming/) — customize the look and feel
- [Auto-Dispatch](/planeai/guides/auto-dispatch/) — orchestrate tasks automatically
