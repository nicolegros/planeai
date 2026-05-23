# ADR-0004: Tauri + Svelte replaces native Swift stack

## Status

Accepted

## Context

planeai was originally a native macOS app: SwiftUI shell, AppKit terminal core, vendored Ghostty for GPU-accelerated rendering (ADR-0001, ADR-0003). This worked but had significant costs:

1. **Platform lock-in** — macOS only, no path to Linux/Windows without a full rewrite.
2. **Ghostty maintenance burden** — vendored fork required rebasing on upstream, building from source with Zig toolchain, and adapting to internal API changes.
3. **Slow iteration on UI** — SwiftUI/AppKit hybrid required bridging code, and the responder chain made keyboard routing complex.
4. **Limited AI-assistance** — Swift/AppKit has less tooling support and smaller training corpus for AI coding agents compared to TypeScript/Rust.

Alternatives considered:

- **Electron + React** — proven but heavy (memory, bundle size). Electron's process model is overkill for a single-window app.
- **Tauri v2 + Svelte 5** — Rust backend (small binary, low memory), web frontend (fast iteration), xterm.js for terminal rendering (mature, WebGL-accelerated).
- **Continue native, add Ghostty public API later** — delays cross-platform indefinitely, keeps maintenance burden.

## Decision

Rewrite planeai using Tauri v2 (Rust backend) + Svelte 5 (frontend) + xterm.js (terminal). Drop Ghostty, keep tmux as the persistence layer (ADR-0002 unchanged).

## Consequences

- Cross-platform becomes achievable (Linux first, Windows later).
- UI iteration speed increases significantly (Svelte + Tailwind + hot reload).
- Terminal rendering quality is slightly lower than Ghostty (xterm.js WebGL vs Metal) but acceptable.
- Rust backend handles tmux, SQLite, and PTY management — well-suited to the task.
- Zig toolchain no longer required in the build.
- The app is no longer a "native" macOS citizen (no native menu bar, dock integration requires Tauri plugins).
- ADR-0001 (Ghostty embedding) and ADR-0003 (SwiftUI+AppKit hybrid) are superseded.
