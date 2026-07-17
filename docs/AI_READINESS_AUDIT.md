# AI-Readiness Audit Report

**Repository:** planeai  
**Date:** 2026-07-17  
**Mode:** Local repo (commands executed)  
**Phases covered:** 1, 2, 3, 5, 6 (Phase 4 Copilot and Phase 7 Compliance skipped)

---

## [Phase 1] — Discovery & Assessment

**Rating:** `Partial`

### Findings

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| 1 | `AGENTS.md` exists at root — covers TDD, performance, commits, and documentation conventions. Concise (30 lines). | — | `[READ]` |
| 2 | `CONTEXT.md` is excellent (352 lines) — glossary, architecture, lifecycle, key constraints, cross-platform strategy, performance rules. | — | `[READ]` |
| 3 | `CONTRIBUTING.md` exists — covers prerequisites, setup, testing, commit conventions, PR expectations. | — | `[READ]` |
| 4 | No issue templates (`.github/ISSUE_TEMPLATE/` missing). Agents cannot self-scope tasks from templates. | Medium | `[READ]` |
| 5 | No PR template (`.github/pull_request_template.md` missing). Agents lack structured checklist for PR descriptions. | Medium | `[READ]` |
| 6 | No path-specific instruction files (no nested `AGENTS.md` in subsystems). | Low | `[READ]` |
| 7 | CI workflows are fast and path-scoped — TypeScript CI runs lint+test+svelte-check; Rust CI runs fmt+clippy+test. Both pass. | — | `[RUN]` |
| 8 | `Makefile` provides `ci`, `test`, `lint`, `fmt` targets — well-documented, all work. | — | `[RUN]` |
| 9 | `.env.example` only documents 2 vars (`JIRA_CLIENT_ID`, `JIRA_CLIENT_SECRET`). Code uses ~15 additional env vars (`PLANEAI_SESSION_LOG_DIR`, `PLANEAI_DAEMON_PTY_CORE`, `PLANEAI_BENCH_*`, `COPILOT_HOME`, `RUST_LOG`, etc.) that are undocumented. | Medium | `[READ]` |
| 10 | No dependency update tooling (no Dependabot or Renovate config). | Medium | `[READ]` |
| 11 | Commit history uses conventional commits consistently. PR titles follow `type(scope): description (#N)` pattern. | — | `[READ]` |

### Recommendations

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | Add `.github/pull_request_template.md` with checklist (tests pass, CONTEXT.md updated, TDD followed) | Low |
| 2 | Add issue template(s) in `.github/ISSUE_TEMPLATE/` (bug, feature, task) | Low |
| 3 | Document all env vars in `.env.example` with inline comments | Low |
| 4 | Configure Dependabot or Renovate for automated dependency updates | Low |

**Summary:** Strong foundation — the repo has excellent architectural documentation (CONTEXT.md) and working CI. Main gaps are task discovery infrastructure (templates) and automated dependency management.

---

## [Phase 2] — Instruction Files & Documentation

**Rating:** `Partial`

### Findings

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| 1 | `AGENTS.md` is too thin — 5 sections covering TDD, performance, commits, documentation. Missing: project structure overview, full command reference, code style expectations, test patterns, error handling conventions. | Medium | `[READ]` |
| 2 | `CONTEXT.md` is excellent for architecture but not formatted as an instruction file — it's a reference doc for domain concepts, not actionable build/test/style instructions. | Low | `[READ]` |
| 3 | No path-specific instructions for major subsystems. `src-tauri/` (Rust backend), `src/` (Svelte frontend), and `src-tauri/planeai-core/` (shared library) have meaningfully different conventions. | Medium | `[READ]` |
| 4 | Undocumented knowledge: (a) oxlint is the linter (not eslint); (b) oxfmt is the formatter; (c) `commands::blocking()` pattern for Tauri commands; (d) Svelte 5 runes (not stores); (e) bits-ui for complex interactives; (f) test co-location in `__tests__/` directories. | Medium | `[READ]` |
| 5 | AGENTS.md doesn't reference the `Makefile` — agents may not discover `make ci` or `make lint` as the canonical commands. | Low | `[READ]` |
| 6 | No documentation of the PR review expectations beyond "write tests" and "clear description". | Low | `[READ]` |

### Recommendations

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | Expand `AGENTS.md` to include: full command reference (`make ci`, `pnpm test`, `cargo test`), project structure map, code style rules (oxlint, oxfmt, cargo fmt, cargo clippy), test patterns | Medium |
| 2 | Add `src-tauri/AGENTS.md` covering: `commands::blocking()` pattern, `async` requirement for all Tauri commands, `no_window()` on Windows, Mutex rules, error handling (`anyhow`), module organization | Low |
| 3 | Add `src/AGENTS.md` covering: Svelte 5 runes (no stores), bits-ui usage, design system tokens, typed API layer (ADR-0009), test setup with jsdom + vitest | Low |
| 4 | Document env vars comprehensively — split into "required for build", "optional for runtime", "debug/bench only" | Low |

**Summary:** The repo has great domain documentation but the agent instruction surface is thin. An agent can build the project but will miss important conventions (oxlint not eslint, Svelte 5 runes not stores, blocking() pattern) without reading ~400 lines of CONTEXT.md.

---

## [Phase 3] — Coding Standards & Conventions

**Rating:** `Partial`

### Findings

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| 1 | **Linting/formatting is well-configured and enforced in CI:** oxlint (TS), oxfmt (TS), cargo fmt (Rust), cargo clippy (Rust), svelte-check (types). All pass. | — | `[RUN]` |
| 2 | **No pre-commit hooks** — agents and humans can push code without local validation. CI catches errors but feedback is delayed. | Medium | `[READ]` |
| 3 | **No dependency update tool** — 4 Rust audit vulnerabilities (quick-xml, serde_yml) and 13 npm audit vulnerabilities (vitest critical, vite high ×2). Patches available for all. | High | `[RUN]` |
| 4 | **Implicit conventions not codified:** (a) File naming: kebab-case for TS/Svelte, snake_case for Rust; (b) Test naming: `describe`/`it` for vitest, `#[test] fn test_*` for Rust; (c) State management: `.svelte.ts` suffix for reactive stores; (d) Component structure: logic in `src/lib/`, UI in `src/components/`, primitives in `src/components/ui/`; (e) Error handling: `anyhow` in Rust, no consistent pattern in TS. | Medium | `[READ]` |
| 5 | **CI provides fast feedback** — TypeScript CI is ~2 min (lint + test + svelte-check). Rust CI is ~3 min (fmt + clippy + test). Well under the 15-min threshold. | — | `[RUN]` |
| 6 | **CONTRIBUTING.md doesn't document agent workflow** — how to pick up a task, iterate with validation, self-review before submitting. | Low | `[READ]` |
| 7 | **`auto` (intuit) configured for releases** — conventional commits drive semver bumps. Well-documented in `.autorc`. | — | `[READ]` |
| 8 | **No test flakiness detection** — no retry mechanisms or quarantine labels visible in CI or issues. | Low | `[READ]` |
| 9 | **Product analytics / experiment infrastructure: N/A** — desktop app, no telemetry. | — | `[READ]` |

### Recommendations

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | **Fix critical/high vulnerabilities immediately** — bump vitest to ≥3.2.6, vite to ≥6.3.6 / ≥6.4.3. Replace `serde_yml` with maintained alternative (`serde_yaml_ng` or `serde_yaml`). Update `quick-xml`. | Low |
| 2 | **Add Dependabot** with weekly schedule for npm and Cargo ecosystems | Low |
| 3 | **Add pre-commit hooks** (lefthook or husky) running `make lint` to catch issues before push | Low |
| 4 | **Document implicit conventions** in AGENTS.md — file naming, test patterns, state management suffix, component hierarchy | Low |
| 5 | Add agent workflow section to CONTRIBUTING.md | Low |

**Summary:** Enforcement tooling is solid (CI gates are comprehensive and fast). The main gaps are dependency freshness (known vulnerabilities with available patches) and local validation (no pre-commit hooks). Implicit conventions are discoverable from code but not documented.

---

## [Phase 5] — Structure & Discoverability

**Rating:** `Partial`

### Findings

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| 1 | **Naming is clear and predictable** — `src-tauri/` for backend, `src/` for frontend, `src/lib/` for logic, `src/components/` for UI, `docs/` for docs, `bench/` for benchmarks. Snake_case Rust, kebab-case TS. | — | `[READ]` |
| 2 | **Excessively large files that hurt agent context windows:** | High | `[READ]` |
|   | `recipe_tick.rs` — 2656 lines | | |
|   | `session_ops.rs` — 1753 lines | | |
|   | `bin/cli.rs` — 1446 lines | | |
|   | `axi/loop_cmds.rs` — 1098 lines | | |
|   | `db.rs` — 1095 lines | | |
|   | `commands/pr.rs` — 1005 lines | | |
|   | `ReviewTab.svelte` — 984 lines | | |
|   | `UnifiedSidebar.svelte` — 953 lines | | |
|   | `App.svelte` — 807 lines | | |
| 3 | **Test file is enormous:** `axi/tests.rs` — 3842 lines. Acceptable for test files but could be split per command. | Low | `[READ]` |
| 4 | **No README in `src-tauri/planeai-core/`** — this is the shared library crate used by all other crates; its purpose and module structure aren't documented. | Medium | `[READ]` |
| 5 | **No `.devcontainer/`** — new contributors must manually install Rust, Node 20, pnpm, and platform-specific deps. | Medium | `[READ]` |
| 6 | **No `docker-compose.yml`** — acceptable since there are no external service dependencies (SQLite is embedded). | — | `[READ]` |
| 7 | **`.env.example` exists but is minimal** (2 vars). Many runtime env vars are undocumented. | Medium | `[READ]` |
| 8 | **Structured logging is configured** — `tracing` crate with rolling file appender, `RUST_LOG` filter. Good for debugging. | — | `[READ]` |
| 9 | **No distributed tracing or metrics** — acceptable for a desktop app, not a service. | — | `[READ]` |
| 10 | **Documentation completeness:** A fresh clone can build and test following README → CONTRIBUTING.md alone. Verified. | — | `[RUN]` |
| 11 | **Tests are co-located** — `__tests__/` directories inside `src/lib/` and `src/components/`. Predictable mirrored structure for Rust tests (inline + test modules). | — | `[READ]` |

### Recommendations

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | **Split `recipe_tick.rs`** (2656 lines) — extract state machine steps, context builders, and step executors into submodules | High |
| 2 | **Split `session_ops.rs`** (1753 lines) — separate session CRUD, worktree management, and backend-specific logic | High |
| 3 | **Split `App.svelte`** (807 lines) — extract layout manager, keyboard orchestration, and tab routing into composable modules | Medium |
| 4 | Add `src-tauri/planeai-core/README.md` explaining the crate's role, module map, and public API | Low |
| 5 | Expand `.env.example` with all env vars (documented with comments and categorized) | Low |
| 6 | Consider adding a `.devcontainer/` for one-click onboarding | Medium |

**Summary:** Structure is logical and naming is excellent. The biggest friction for agents is oversized files — 9 source files exceed 500 lines, with the largest at 2656 lines. These files are hard for agents to hold in context and reason about. Splitting them is the highest-impact structural improvement.

---

## [Phase 6] — Security

**Rating:** `Partial`

### Findings

| # | Finding | Severity | Evidence |
|---|---------|----------|----------|
| 1 | **No secrets in instruction files** — AGENTS.md, CONTEXT.md, and CONTRIBUTING.md reference env vars by name only, never by value. | — | `[RUN]` (grep scan) |
| 2 | **No prompt injection risks** — instruction files contain no override directives, no hidden Unicode, no instructions to bypass safety. | — | `[READ]` |
| 3 | **No data exfiltration commands** — no curl/wget/fetch to external URLs in instruction files or Makefile. | — | `[READ]` |
| 4 | **No external scripts fetched at runtime** — all dependencies are from package registries (npm, crates.io). No `curl | sh` patterns. | — | `[READ]` |
| 5 | **Known vulnerabilities exist and are unpatched:** | High | `[RUN]` |
|   | Rust: 4 advisories — `quick-xml` (2× DoS), `serde_yml` (2× unsound/unmaintained) | | |
|   | npm: 13 vulnerabilities — `vitest` (critical: arbitrary file read/exec), `vite` (2× high: file read via WebSocket, `server.fs.deny` bypass) | | |
| 6 | **No automated security scanning** — no CodeQL, Snyk, Trivy, or equivalent in CI workflows. | High | `[READ]` |
| 7 | **No security posture measurement** — no OpenSSF Scorecard or similar. | Low | `[READ]` |
| 8 | **Release workflow uses pinned actions** (`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`) — acceptable supply chain hygiene. | — | `[READ]` |
| 9 | **CI uses `GITHUB_TOKEN` with minimal scope** — `contents: write`, `pull-requests: read`, `issues: read` for release only. | — | `[READ]` |
| 10 | **Agent permission boundaries:** No tool-specific configs (`.kiro/settings.json`, `.cursor/rules`, etc.) that restrict destructive operations. Acceptable for early-stage personal project. | Low | `[READ]` |

### Recommendations

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | **Patch critical npm vulns** — `vitest` ≥3.2.6, `vite` ≥6.3.6 | Low |
| 2 | **Replace `serde_yml`** — unmaintained, unsound. Use `serde_yaml_ng` or `serde_yaml` | Medium |
| 3 | **Add `cargo audit` and `pnpm audit` to CI** — fail on critical/high severity | Low |
| 4 | **Add Dependabot** for automated patch PRs (see Phase 3) | Low |
| 5 | Consider adding CodeQL or similar for static analysis in CI | Low |

**Summary:** No secrets or injection risks — the instruction files are clean. The main security gap is unpatched dependencies with known critical/high vulnerabilities and no automated scanning to catch them going forward.

---

## Overall Readiness Summary

| Area | Status | Notes |
|------|--------|-------|
| Root instruction file | ✅ Exists | Needs expansion |
| Build/test/lint commands documented | ✅ | Makefile + CONTRIBUTING.md |
| Commands verified working | ✅ | All pass |
| Project structure documented | ✅ | CONTEXT.md is comprehensive |
| Path-specific instructions | ❌ Missing | Frontend and backend have different conventions |
| Task discovery (templates) | ❌ Missing | No issue or PR templates |
| Coding standards enforced | ✅ | CI gates: oxlint, cargo clippy, fmt, svelte-check |
| Contributing workflow documented | ✅ | Could be richer |
| Dependencies up to date | ❌ | Critical/high vulns unpatched, no update tool |
| CI fast feedback | ✅ | <5 min combined |
| Code complexity | ⚠️ | 9 files >500 lines (largest: 2656) |
| Dev environment reproducible | ⚠️ | Manual setup only (no devcontainer) |
| No secrets in instruction files | ✅ | Clean |
| Security scanning active | ❌ | No automated scanning in CI |
| Local validation | ⚠️ | No pre-commit hooks |

### Top 5 Actions (prioritized by agent-impact × effort)

1. **Patch critical dependency vulnerabilities** — bump vitest/vite, replace serde_yml (Low effort, High impact)
2. **Expand AGENTS.md** — add command reference, project structure, code style rules, test patterns (Medium effort, High impact)
3. **Add PR template + issue templates** — structured intake for agents (Low effort, Medium impact)
4. **Add Dependabot + audit in CI** — automated security hygiene (Low effort, Medium impact)
5. **Add pre-commit hooks** (lefthook) — local validation before push (Low effort, Medium impact)

### Deferred (high effort, lower urgency)

- Split oversized files (recipe_tick.rs, session_ops.rs, App.svelte)
- Add .devcontainer for reproducible onboarding
- Add path-specific AGENTS.md for frontend/backend subsystems
