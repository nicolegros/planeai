# Design System

planeai's visual language is **monochrome-first**. The accent is near-black in light mode, near-white in dark mode — not a brand color. Chromatic color appears only in status indicators, terminal ANSI, and error/warning states.

## Architecture

The design system lives in two layers:

1. **Static `@theme` block** (`src/app.css`) — Tailwind v4 token definitions. These are compile-time values for the light theme baseline.
2. **Runtime `<style id="planeai-theme">`** — Injected by `src/lib/theme-loader.ts`. Overrides `--color-*` custom properties for dark mode and custom themes.

**Important:** The `@theme` block must contain static values only — no `var()` references. Runtime theme switching works by overriding the same `--color-*` properties via the injected stylesheet.

Custom user themes are plain CSS files in `~/.config/planeai/themes/` that override these same properties.

## Color Tokens

### Role-based tokens (what you use in components)

| Token       | Purpose                           | Light                 | Dark                     |
| ----------- | --------------------------------- | --------------------- | ------------------------ |
| `canvas`    | App chrome/sidebar background     | `#f4f4f6`             | `#171717`                |
| `chrome`    | Titlebar, helper bar              | `#f4f4f6`             | `#171717`                |
| `sidebar`   | Sidebar background                | `#f4f4f6`             | `#171717`                |
| `main`      | Content/terminal/diff background  | `#ffffff`             | `#0a0a0a`                |
| `panel`     | Cards, modals, dropdowns          | `#ffffff`             | `#1f1f23`                |
| `panel-hi`  | Inputs, hover states, badges      | `#e9e9ed`             | `#2c2c31`                |
| `t1`        | Primary text                      | `#18181d`             | `#f2f2f2`                |
| `t2`        | Secondary text                    | `#686870`             | `#9e9e9e`                |
| `t3`        | Muted text, placeholders          | `#88888f`             | `#5b5b61`                |
| `border`    | Default borders                   | `rgba(0,0,0,0.06)`    | `rgba(255,255,255,0.08)` |
| `border-s`  | Strong borders (modals, cards)    | `rgba(0,0,0,0.10)`    | `rgba(255,255,255,0.14)` |
| `accent`    | Primary interactive color         | `#18181d`             | `#f5f5f5`                |
| `on-accent` | Text on accent backgrounds        | `#ffffff`             | `#0a0a0a`                |
| `accent-bg` | Active rows, selected chips       | `rgba(24,24,29,0.07)` | `rgba(245,245,245,0.12)` |
| `scrim`     | Modal backdrop (currently unused) | `rgba(25,25,30,0.30)` | `rgba(0,0,0,0.55)`       |

### Status colors

| Token            | Purpose                    | Light     | Dark      |
| ---------------- | -------------------------- | --------- | --------- |
| `status-running` | Running/success/green      | `#1a7f37` | `#3fb950` |
| `status-review`  | Needs review/warning/amber | `#9a6700` | `#d29922` |
| `status-exited`  | Exited/error/red           | `#cf222e` | `#ff7b72` |
| `status-idle`    | Idle sessions              | `#aeaeb5` | `#5b5b61` |

### Usage in Tailwind

```svelte
<!-- Background -->
<div class="bg-panel">...</div>
<div class="bg-panel-hi">...</div>
<div class="bg-accent-bg">...</div>

<!-- Text -->
<span class="text-t1">Primary</span>
<span class="text-t2">Secondary</span>
<span class="text-t3">Muted</span>

<!-- Borders -->
<div class="border border-border">...</div>
<div class="border border-border-s">...</div>

<!-- Accent -->
<button class="bg-accent text-on-accent">...</button>
```

**No `dark:` variants needed** — the runtime theme injection handles mode switching. Use `dark:` only for static Tailwind colors (`red-*`, `amber-*`, etc.) that aren't part of the design system.

## Typography

| Role                 | Font              | Weight      | Size                         |
| -------------------- | ----------------- | ----------- | ---------------------------- |
| UI text              | IBM Plex Sans     | 400/500/600 | 12–13.5px                    |
| Titlebar             | IBM Plex Sans     | 500         | 12.5px                       |
| Sidebar session name | IBM Plex Sans     | 500         | 13px                         |
| Status group labels  | IBM Plex Sans     | 600         | 9.5–11px uppercase, ls .05em |
| Code/mono            | IBM Plex Mono     | 400/500     | 12.5px                       |
| Keyboard badges      | IBM Plex Mono     | —           | 10px                         |
| Terminal             | User-configurable | —           | User-configurable            |

Tailwind classes: `font-sans` for UI, `font-mono` for code/metadata.

## Radii

| Token | Value | Use                         |
| ----- | ----- | --------------------------- |
| `sm`  | 6px   | Inputs, badges, small chips |
| `md`  | 8px   | Buttons, cards              |
| `lg`  | 12px  | Modals, panels              |

## Geometry

| Element             | Size                    |
| ------------------- | ----------------------- |
| Titlebar            | 38px height             |
| Sidebar             | 264–266px default width |
| Keyboard helper bar | 34px height             |
| Command palette     | 600px                   |
| New session modal   | 452px                   |
| PR panel            | 282px                   |
| Tab switcher        | 512px                   |

## Icons

All icons from `@lucide/svelte`. Key icons:

- **Tabs:** `Bot` (Agent), `Terminal` (Shell), `GitCompare` (Diff), `FileCode` (Editor)
- **Agent state:** `LoaderCircle` (busy, animate-spin), `Lightbulb` (needs attention, animate-pulse)
- **CI:** `CheckCircle2` (pass), `XCircle` (fail), `LoaderCircle` (running)
- **PR:** `GitPullRequest`, `GitMerge`
- **Navigation:** `ChevronDown`, `ChevronRight`, `Plus`, `Settings`
- **Auto-dispatch:** `Zap`

## Components

### Shared primitives (`src/components/ui/`)

| Component      | Purpose                                          |
| -------------- | ------------------------------------------------ |
| `Button`       | Primary/ghost/danger variants                    |
| `Input`        | Text input with `border-border bg-panel text-t1` |
| `Select`       | Combobox dropdown                                |
| `Checkbox`     | Labeled checkbox                                 |
| `Dialog`       | Centered modal (no backdrop)                     |
| `ContextMenu`  | Right-click menu                                 |
| `Label`        | Form label                                       |
| `ResizeHandle` | Panel resize drag handle                         |

### Focus indicators

- **Sidebar focused:** Right border changes to `border-accent`
- **Sidebar cursor (keyboard nav):** `ring-2 ring-accent` on the selected item
- **Session preview (Cmd/Ctrl+{/}):** `ring-2 ring-accent` on the previewed item
- **Active session:** `bg-accent-bg` + 2px left accent bar

### Modals

All modals use `ui/Dialog` or the same pattern: centered, no backdrop overlay, `border-border-s bg-panel shadow-lg`.

### Form keyboard mode indicator

Forms that use the form keyboard controller display a mode badge and mnemonic hints:

- **Mode badge:** `NORMAL` (`bg-panel-hi text-t2`) or `INSERT` (`bg-accent-bg text-accent`), 10px mono uppercase.
- **Mnemonic badges:** Inline `<span>` next to each label. In normal mode: `bg-accent-bg text-accent`; in insert mode: `bg-panel-hi text-t3`.
- **Submit hint:** `MOD_ENTER_HINT` (⌘↵ / Ctrl+↵) shown inside the primary submit button at reduced opacity.

### Form submission loading state

All forms and async action buttons follow a consistent loading pattern:

- A `submitting` reactive state (`$state(false)`) guards against double-submission.
- The submit button is `disabled` while submitting.
- Button label is replaced with a `LoaderCircle` spinner (`size-3.5 animate-spin`) during submission.
- On error, `submitting` resets to `false` so the user can retry. On success, the form closes (no reset needed).
- `Cmd/Ctrl+Enter` shortcuts also check `submitting` before invoking submit.

## Motion

Minimal, purposeful animation only:

- Running dot: `pulse-dot 1.6s ease-in-out infinite`
- Idle lightbulb: `animate-pulse` (~1.8s)
- Busy spinner: `animate-spin` (0.7s linear)
- Terminal caret: `blink-caret 1.1s`
- Selection changes: instant or 120–150ms transition
- Sidebar archive/delete: 200ms opacity fade-out before action executes

No decorative animation.
