# 8. Custom theming with CSS custom properties + bits-ui

Date: 2026-06-04

## Status

Accepted (supersedes ADR-0005)

## Context

ADR-0005 chose `@skeletonlabs/skeleton` for the token/theming layer and `bits-ui` for interactive primitives. In practice, Skeleton added ~220 lines of generated CSS variables (most unused), required a `data-theme` attribute, and prevented users from supplying their own themes. We needed user-customizable theming covering UI, terminal, and code editor colors.

## Decision

- **Remove `@skeletonlabs/skeleton`** entirely. Own the token layer via a Tailwind v4 `@theme` block in `app.css` that maps CSS custom property names to utility classes.
- **Theme files are plain CSS** with `:root` (light) and `.dark` (dark) blocks defining the full token contract: `surface-{50–950}`, `primary-{50–950}`, `error-{50–950}`, `warning-{50–500}`, terminal 16-color ANSI palette, and editor colors.
- **Theme location**: `~/.config/planeai/themes/{name}.css`. Config field `theme = "default"` (name only, no extension).
- **Loading**: Tauri `get_theme_css` command returns the CSS string. Frontend injects a `<style id="planeai-theme">` tag. Hot-reloads on theme name change.
- **Embedded fallback**: `default.css` is bundled via `include_str!` and scaffolded to disk on first launch. App never renders unstyled.
- **Terminal colors**: Theme provides `--terminal-*` CSS vars. Frontend reads them via `getComputedStyle` and builds the xterm `ITheme` object. Existing hardcoded presets in `terminal-themes.ts` remain as config-level overrides.
- **Editor colors**: CodeMirror theme uses `var(--editor-*)` directly in style specs — no JS reconstruction needed, cascades automatically on theme change.
- **bits-ui** remains the only component library for complex interactives (Dialog, Command, Combobox). Simple primitives (Button, Input, Label, Checkbox) stay hand-rolled in `src/components/ui/`.

## Consequences

- Users can create and share theme files. Writing a theme requires defining 3 required color scales (surface, primary, error) + terminal/editor tokens.
- All 191 existing Tailwind utility usages (`bg-surface-*`, `text-primary-*`) continue working with zero renames.
- One fewer devDependency. Theme file is ~130 lines vs Skeleton's ~220.
- Terminal and editor colors are unified with UI theme — one file controls the entire visual appearance.
- Config-level `terminal_theme_dark`/`terminal_theme_light` fields override the theme file's terminal colors for power users who want to mix.
