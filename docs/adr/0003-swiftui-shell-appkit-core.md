# ADR-0003: SwiftUI shell with AppKit terminal core

## Status

Accepted

## Context

planeai is a keyboard-first app that embeds terminal surfaces. The framework choice affects keyboard handling, focus management, and rendering integration.

1. **Pure AppKit** — full control over responder chain and key events, but verbose for UI chrome (sidebar, palettes, settings).
2. **Pure SwiftUI** — modern and concise, but keyboard/focus handling (`@FocusState`) is too limited for power-user terminal apps. Embedding NSView-backed surfaces requires bridging.
3. **SwiftUI shell + AppKit core** — SwiftUI for chrome (sidebar, command palette, settings), AppKit (NSView subclasses) for terminal surfaces and keyboard event routing.

## Decision

Hybrid: SwiftUI for the application shell (sidebar, project list, command palette, settings), AppKit for terminal surfaces and keyboard routing. This mirrors Ghostty's own macOS app architecture.

## Consequences

- SwiftUI provides fast iteration on UI chrome with modern APIs (`@Observable`, etc.).
- AppKit gives precise control over `keyDown`, first responder management, and custom key routing.
- `Cmd+` shortcuts intercepted at the AppKit level; all other input passes to the terminal.
- Requires `NSViewRepresentable` bridges between SwiftUI and AppKit views.
- macOS 14 minimum enables `@Observable` macro for cleaner state management.
