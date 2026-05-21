# planeai

A native macOS app for running multiple AI coding agents in parallel. Each agent works in an isolated git worktree, backed by tmux for session persistence, with GPU-accelerated terminal rendering via ghostty.

## Features

- **Project-based organization** — register git repos as projects with default settings
- **Session management** — launch agents, split panes, archive/delete sessions
- **Multi-provider** — Claude Code, Kiro, Codex, Copilot, and custom agents
- **Worktree isolation** — each session gets its own git worktree automatically
- **tmux persistence** — agents keep running when you quit the app
- **Notifications** — sidebar badges, macOS notifications, and unread queue when agents need attention
- **100% keyboard-driven** — every action reachable without a mouse

## Requirements

- macOS 14 (Sonoma) or later
- tmux (`brew install tmux`)
- At least one supported agent CLI on PATH

## Architecture

- Swift/AppKit + SwiftUI hybrid
- Ghostty (vendored submodule) for terminal rendering
- tmux for process persistence
- Internal SPM packages for modularity

See [CONTEXT.md](./CONTEXT.md) for domain glossary and [docs/adr/](./docs/adr/) for architecture decisions.

## Development

This project uses TDD. See [AGENTS.md](./AGENTS.md) for development workflow guidelines.
