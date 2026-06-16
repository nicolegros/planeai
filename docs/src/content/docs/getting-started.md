---
title: Getting Started
description: Install planeai and run your first AI agent session.
draft: false
---

## Install

Download the latest release for your platform:

| Platform              | Format                                                                                                                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| macOS (Apple Silicon) | [`.dmg`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |
| Linux                 | [`.deb`](https://github.com/nicolegros/planeai/releases/latest) / [`.AppImage`](https://github.com/nicolegros/planeai/releases/latest) |
| Windows               | [`.exe`](https://github.com/nicolegros/planeai/releases/latest)                                                                        |

## Requirements

- **tmux** (optional, for persistent sessions) — `brew install tmux` on macOS
- **At least one AI agent CLI on PATH** — e.g., `kiro-cli`, `claude`, `gh copilot`

## Basic Usage

Launch planeai after installation. Use **Cmd+K** (macOS) or **Ctrl+K** (Linux/Windows) to open the command menu and create your first session.

## Development

To run planeai from source:

```bash
pnpm install
pnpm tauri dev
```

## Next Steps

- [Configuration](/planeai/guides/configuration/) — set up providers, templates, and hooks
- [Theming](/planeai/guides/theming/) — customize the look and feel
- [Auto-Dispatch](/planeai/guides/auto-dispatch/) — orchestrate tasks automatically
