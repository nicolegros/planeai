# Contributing

Thanks for considering a contribution to planeai!

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- [pnpm](https://pnpm.io/) 10+
- [tmux](https://github.com/tmux/tmux) (optional, for persistent session testing)
- macOS: Xcode Command Line Tools
- Linux: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`

## Setup

```bash
git clone https://github.com/nicolegros/planeai.git
cd planeai
pnpm install
```

## Development

```bash
pnpm tauri dev     # Run the app in dev mode
```

## Testing

```bash
pnpm test              # Frontend (vitest)
cd src-tauri && cargo test   # Backend (Rust)
```

Or run both with:

```bash
make test
```

## Commit Convention

Use [conventional commits](https://www.conventionalcommits.org/):

- `feat:` — new feature
- `fix:` — bug fix
- `docs:` — documentation only
- `refactor:` — code change that neither fixes a bug nor adds a feature
- `test:` — adding or updating tests
- `chore:` — maintenance (deps, CI, etc.)

## Pull Requests

1. Fork the repo and create a branch from `main`
2. Write tests for new functionality (TDD preferred — see [AGENTS.md](./AGENTS.md))
3. Ensure `pnpm test` and `cargo test` pass
4. Open a PR with a clear description of what and why

## Architecture

- [CONTEXT.md](./CONTEXT.md) — domain glossary and architecture notes
- [docs/adr/](./docs/adr/) — architecture decision records
- [docs/configuration.md](./docs/configuration.md) — full configuration reference
