# ADR-0001: Embed ghostty via vendored fork with internal C API

## Status

Accepted

## Context

planeai needs GPU-accelerated terminal rendering. The options were:

1. **Use the public `libghostty-vt` API** — only provides parsing/state, no rendering. Would require building our own Metal renderer.
2. **Wait for the public libghostty embedding API** — not shipped yet, uncertain timeline.
3. **Vendor Ghostty as a submodule and use the internal C API** — the same approach cmux (17k+ stars) uses in production. The internal API is "messy" per Mitchell Hashimoto but functional and proven.
4. **Build a custom terminal emulator** — massive scope, no benefit.

## Decision

Fork `ghostty-org/ghostty`, add as a git submodule, and use the internal C API via a bridging header. Keep patches minimal and rebased on upstream. Migrate to the official public embedding API when it ships.

## Consequences

- We get full GPU-accelerated terminal rendering, input handling, and scrollback immediately.
- We inherit Ghostty's config format for appearance (font, theme, colors).
- We take on the maintenance burden of keeping the fork rebased on upstream.
- Breaking changes in Ghostty's internal API may require adaptation work.
- The app requires building Ghostty from source (Zig toolchain needed in CI).
