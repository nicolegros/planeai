# 5. Skeleton tokens + bits-ui for the design system

Date: 2026-05-24

## Status

Accepted

## Context

The UI had no design system — components used hardcoded Tailwind color classes (`bg-gray-50`, `text-gray-600`) with no shared tokens or reusable primitives. Two component libraries were installed: `@skeletonlabs/skeleton-svelte` (full component set) and `bits-ui` (headless primitives). Only bits-ui was actually in use (Dialog, Command).

We needed a consistent visual language, theme support (light + dark), and a small set of reusable UI primitives.

## Decision

- Use **`@skeletonlabs/skeleton`** (CSS-only package) for the design token layer. Theme: Cerberus. Provides semantic CSS custom properties and a Tailwind plugin for utility classes like `bg-surface-200`, `text-primary-500`.
- Use **`bits-ui`** for all interactive component behavior (Dialog, Command, Select, etc.). Style these with Skeleton token classes.
- **Drop `@skeletonlabs/skeleton-svelte`** — no Skeleton Svelte components. Single component API (bits-ui) avoids conflicting composition patterns.
- Build a thin **`src/components/ui/`** layer of styled wrappers around bits-ui that encode the visual language (Button, Input, Select, Dialog, Label, Checkbox, ContextMenu).
- Support **light and dark themes** with a three-state user preference (system / light / dark), persisted in localStorage.

## Consequences

- All components reference semantic tokens, not hardcoded colors. Changing the theme or palette is a single-file change.
- Two packages serve distinct roles: Skeleton = tokens/theming, bits-ui = behavior/accessibility. No overlap.
- Adding a new UI primitive means creating a wrapper in `ui/` that composes bits-ui + token classes. Documented process in CONTEXT.md.
- Skeleton's Svelte component library is unused dead weight and removed.
