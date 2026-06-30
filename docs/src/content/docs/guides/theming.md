---
title: Theming
description: Customize planeai's appearance with CSS theme files.
---

Themes are CSS files that define custom properties controlling the entire UI — colors, radii, terminal palette, editor highlighting, and diff colors. planeai ships with several bundled themes and supports custom themes.

## Selecting a theme

Open **Preferences** (⌘, / Ctrl+,) and look under the **Appearance** section. Available themes appear as clickable buttons — select one to apply it immediately.

Alternatively, set the theme in your config file:

```jsonc
{
  "appearance": {
    "theme": "catppuccin",
  },
}
```

## Theme file location

Themes live in `~/.config/planeai/themes/` (or `%APPDATA%\planeai\themes\` on Windows). On first launch, planeai scaffolds this directory with the bundled themes. Any `.css` file you add here becomes available in the theme selector.

## Bundled themes

| Theme        | Description                                 |
| ------------ | ------------------------------------------- |
| `default`    | Monochrome design system (GitHub-inspired)  |
| `github`     | GitHub Light / Dark                         |
| `catppuccin` | Catppuccin Latte (light) / Macchiato (dark) |
| `dracula`    | Dracula color palette                       |
| `one`        | Atom One Light / Dark                       |

## Creating a custom theme

1. Copy an existing theme as a starting point:
   ```bash
   cp ~/.config/planeai/themes/default.css ~/.config/planeai/themes/my-theme.css
   ```
2. Edit the CSS custom properties in `my-theme.css`
3. Select "my-theme" in **Preferences → Appearance**

## File structure

Each theme defines variables in `:root` (light mode) and `.dark` (dark mode):

```css
:root {
  /* Light mode tokens */
  --color-canvas: #f4f4f6;
  --color-accent: #18181d;
  /* ... */
}

.dark {
  /* Dark mode tokens */
  --color-canvas: #171717;
  --color-accent: #f5f5f5;
  /* ... */
}
```

## Token reference

### UI role tokens

| Token               | Description                                     |
| ------------------- | ----------------------------------------------- |
| `--color-canvas`    | Base page background                            |
| `--color-chrome`    | Chrome/header background                        |
| `--color-sidebar`   | Sidebar background                              |
| `--color-main`      | Main content area background                    |
| `--color-panel`     | Panel/card background                           |
| `--color-panel-hi`  | Elevated panel (hover states, highlights)       |
| `--color-t1`        | Primary text                                    |
| `--color-t2`        | Secondary text                                  |
| `--color-t3`        | Muted text (hints, placeholders)                |
| `--color-border`    | Default border (subtle)                         |
| `--color-border-s`  | Strong border (more visible)                    |
| `--color-accent`    | Primary accent color                            |
| `--color-on-accent` | Text color on accent backgrounds                |
| `--color-accent-bg` | Accent tint background (selections, highlights) |
| `--color-scrim`     | Modal/overlay backdrop                          |

### Status colors

| Token                    | Description                      |
| ------------------------ | -------------------------------- |
| `--color-status-running` | Active/running indicator (green) |
| `--color-status-review`  | Needs review indicator (amber)   |
| `--color-status-exited`  | Error/exited indicator (red)     |
| `--color-status-idle`    | Idle/inactive indicator (gray)   |

### Radii

| Token                | Description                      |
| -------------------- | -------------------------------- |
| `--radius-base`      | Small elements (buttons, inputs) |
| `--radius-container` | Large containers (cards, panels) |

### Terminal colors (16 ANSI + chrome)

| Token                   | Description          |
| ----------------------- | -------------------- |
| `--terminal-background` | Terminal background  |
| `--terminal-foreground` | Terminal foreground  |
| `--terminal-cursor`     | Cursor color         |
| `--terminal-selection`  | Selection background |
| `--terminal-black`      | ANSI black           |
| `--terminal-red`        | ANSI red             |
| `--terminal-green`      | ANSI green           |
| `--terminal-yellow`     | ANSI yellow          |
| `--terminal-blue`       | ANSI blue            |
| `--terminal-magenta`    | ANSI magenta         |
| `--terminal-cyan`       | ANSI cyan            |
| `--terminal-white`      | ANSI white           |
| `--terminal-bright-*`   | Bright variants (×8) |

### Diff colors

| Token              | Description             |
| ------------------ | ----------------------- |
| `--diff-add-bg`    | Added line background   |
| `--diff-del-bg`    | Deleted line background |
| `--diff-add-color` | Added line text color   |
| `--diff-del-color` | Deleted line text color |

### Editor colors

| Token                  | Description                       |
| ---------------------- | --------------------------------- |
| `--editor-background`  | Editor background                 |
| `--editor-foreground`  | Editor text                       |
| `--editor-selection`   | Selection background              |
| `--editor-line-number` | Line number gutter                |
| `--editor-added`       | Added text in editor              |
| `--editor-deleted`     | Deleted text in editor            |
| `--editor-added-bg`    | Added line background in editor   |
| `--editor-deleted-bg`  | Deleted line background in editor |

### Syntax highlighting

| Token                  | Description                         |
| ---------------------- | ----------------------------------- |
| `--editor-keyword`     | Keywords (`if`, `return`, `import`) |
| `--editor-string`      | String literals                     |
| `--editor-comment`     | Comments                            |
| `--editor-number`      | Numeric literals                    |
| `--editor-variable`    | Variables                           |
| `--editor-type`        | Type annotations                    |
| `--editor-function`    | Function names                      |
| `--editor-property`    | Object properties                   |
| `--editor-operator`    | Operators                           |
| `--editor-punctuation` | Brackets, semicolons                |
| `--editor-meta`        | Meta/decorators                     |

## Minimal theme example

```css
/* my-theme.css — Catppuccin-inspired custom theme */

:root {
  --color-canvas: #e6e9ef;
  --color-chrome: #e6e9ef;
  --color-sidebar: #e6e9ef;
  --color-main: #eff1f5;
  --color-panel: #eff1f5;
  --color-panel-hi: #dce0e8;
  --color-t1: #4c4f69;
  --color-t2: #6c6f85;
  --color-t3: #8c8fa1;
  --color-border: rgba(76, 79, 105, 0.08);
  --color-border-s: rgba(76, 79, 105, 0.15);
  --color-accent: #1e66f5;
  --color-on-accent: #eff1f5;
  --color-accent-bg: rgba(30, 102, 245, 0.1);
  --color-scrim: rgba(76, 79, 105, 0.3);
  --color-status-running: #40a02b;
  --color-status-review: #df8e1d;
  --color-status-exited: #d20f39;
  --color-status-idle: #9ca0b0;
  --radius-base: 0.5rem;
  --radius-container: 1rem;
  --terminal-background: #eff1f5;
  --terminal-foreground: #4c4f69;
  --terminal-cursor: #dc8a78;
}

.dark {
  --color-canvas: #1e2030;
  --color-chrome: #1e2030;
  --color-sidebar: #1e2030;
  --color-main: #24273a;
  --color-panel: #24273a;
  --color-panel-hi: #363a4f;
  --color-t1: #cad3f5;
  --color-t2: #a5adcb;
  --color-t3: #6e738d;
  --color-border: rgba(202, 211, 245, 0.08);
  --color-border-s: rgba(202, 211, 245, 0.14);
  --color-accent: #8aadf4;
  --color-on-accent: #24273a;
  --color-accent-bg: rgba(138, 173, 244, 0.12);
  --color-scrim: rgba(0, 0, 0, 0.55);
  --color-status-running: #a6da95;
  --color-status-review: #eed49f;
  --color-status-exited: #ed8796;
  --color-status-idle: #5b6078;
  --terminal-background: #24273a;
  --terminal-foreground: #cad3f5;
  --terminal-cursor: #f4dbd6;
}
```

## Tips

:::tip
Use high contrast between `--color-canvas` and `--color-t1` for readability during long sessions.
:::

:::tip
You only need to override the tokens you want to change. Any token you omit falls back to the default theme's value.
:::

:::note
Theme changes take effect immediately when selected in Preferences. Custom theme file edits require re-selecting the theme or restarting the app.
:::
