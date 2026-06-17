---
title: Theming
description: Customize planeai's appearance with CSS theme files.
draft: false
---

Themes are plain CSS files stored in `~/.config/planeai/themes/`. Each file defines CSS custom properties that control the entire UI.

## Quick Start

1. Copy an existing theme as a starting point:
   ```bash
   cp ~/.config/planeai/themes/dark.css ~/.config/planeai/themes/my-theme.css
   ```
2. Edit the CSS custom properties in `my-theme.css`
3. Set the theme in your config:
   ```jsonc
   {
     "theme": "my-theme",
   }
   ```

## File Structure

Themes define variables in `:root` (light mode) and `.dark` (dark mode):

```css
:root {
  --surface-0: #ffffff;
  --surface-1: #f5f5f5;
  --primary-500: #3b82f6;
}

.dark {
  --surface-0: #0a0a0a;
  --surface-1: #171717;
  --primary-500: #60a5fa;
}
```

## Token Reference

### UI Colors

| Token           | Description                 |
| --------------- | --------------------------- |
| `--surface-0`   | Base background             |
| `--surface-1`   | Elevated surface            |
| `--surface-2`   | Card / panel background     |
| `--surface-3`   | Hover state background      |
| `--primary-500` | Primary accent color        |
| `--primary-600` | Primary hover               |
| `--primary-700` | Primary active/pressed      |
| `--error-500`   | Error / destructive actions |
| `--warning-500` | Warning indicators          |

### Radii

| Token         | Description     |
| ------------- | --------------- |
| `--radius-sm` | Small elements  |
| `--radius-md` | Buttons, inputs |
| `--radius-lg` | Cards, panels   |

### Terminal Colors (16 ANSI + Chrome)

| Token             | Description          |
| ----------------- | -------------------- |
| `--term-black`    | ANSI black           |
| `--term-red`      | ANSI red             |
| `--term-green`    | ANSI green           |
| `--term-yellow`   | ANSI yellow          |
| `--term-blue`     | ANSI blue            |
| `--term-magenta`  | ANSI magenta         |
| `--term-cyan`     | ANSI cyan            |
| `--term-white`    | ANSI white           |
| `--term-bright-*` | Bright variants (×8) |
| `--term-bg`       | Terminal background  |
| `--term-fg`       | Terminal foreground  |
| `--term-cursor`   | Cursor color         |

### Editor Colors

| Token                | Description            |
| -------------------- | ---------------------- |
| `--editor-bg`        | Editor background      |
| `--editor-fg`        | Editor foreground      |
| `--editor-line`      | Current line highlight |
| `--editor-selection` | Selection background   |

### Syntax Highlighting

| Token               | Description      |
| ------------------- | ---------------- |
| `--syntax-keyword`  | Keywords         |
| `--syntax-string`   | String literals  |
| `--syntax-comment`  | Comments         |
| `--syntax-function` | Function names   |
| `--syntax-variable` | Variables        |
| `--syntax-number`   | Numeric literals |
| `--syntax-type`     | Type annotations |

## Minimal Theme Example

```css
:root {
  --surface-0: #1e1e2e;
  --surface-1: #282839;
  --surface-2: #313244;
  --surface-3: #3b3b52;
  --primary-500: #89b4fa;
  --primary-600: #74a0e6;
  --primary-700: #5f8cd2;
  --error-500: #f38ba8;
  --warning-500: #f9e2af;
  --radius-sm: 4px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --term-bg: #1e1e2e;
  --term-fg: #cdd6f4;
  --term-cursor: #f5e0dc;
}
```

## Bundled Themes

| Theme       | Description             |
| ----------- | ----------------------- |
| `dark`      | Default dark theme      |
| `light`     | Clean light theme       |
| `midnight`  | High-contrast dark      |
| `solarized` | Solarized color palette |

## Tips

:::tip
Use high contrast between `--surface-0` and `--term-fg` for readability during long sessions.
:::

:::note
Changes to theme files are picked up on app restart. Hot-reload is planned for a future release.
:::
