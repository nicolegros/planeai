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

### Environment Variables

Copy `.env.example` to `.env` and fill in the values:

```bash
cp .env.example .env
```

| Variable             | Required | Description                                                    |
| -------------------- | -------- | -------------------------------------------------------------- |
| `JIRA_CLIENT_ID`     | No\*     | Atlassian 3LO client ID used for Jira OAuth development        |
| `JIRA_CLIENT_SECRET` | No\*     | Atlassian 3LO secret; provide locally only and never commit it |

\* The build succeeds without these (placeholder values are used), but Jira OAuth will not work at runtime. For `cargo test` and `cargo clippy`, dummy values are passed automatically by the Makefile.

Jira OAuth application credentials are managed by PlaneAI release engineering. Release builds receive `JIRA_CLIENT_ID` and `JIRA_CLIENT_SECRET` through protected GitHub Actions secrets; rotation is published in a new application release (ADR-0011).

## Development

```bash
make dev           # Run the app in dev mode
```

## Testing

```bash
make test  # Frontend and backend tests
make ci    # Full lint and test suite
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
3. Ensure `make ci` passes
4. Open a PR with a clear description of what and why

## Architecture

- [CONTEXT.md](./CONTEXT.md) — domain glossary and architecture notes
- [docs/adr/](./docs/adr/) — architecture decision records
- [docs/configuration.md](./docs/configuration.md) — full configuration reference
