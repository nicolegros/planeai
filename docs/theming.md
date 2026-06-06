# Custom Themes

planeai uses a single CSS file per theme to control the entire visual appearance: UI colors, terminal palette, and code editor syntax highlighting. Themes live in `~/.config/planeai/themes/` and are selected via the `appearance.theme` field in `config.json`.

## Quick Start

1. Copy an existing theme as a starting point:
   ```bash
   cp ~/.config/planeai/themes/default.css ~/.config/planeai/themes/my-theme.css
   ```
2. Edit `my-theme.css` — change any colors you want.
3. Set it as your active theme in `~/.config/planeai/config.json`:
   ```jsonc
   {
     "appearance": {
       "theme": "my-theme",
     },
   }
   ```
4. The theme applies immediately (no restart needed).

## File Structure

A theme file defines CSS custom properties in two blocks:

```css
/* My Theme */

:root {
  /* Light mode values */
}

.dark {
  /* Dark mode values */
}
```

Both blocks are required. The app applies `.dark` to the document root when in dark mode.

## Token Reference

### UI Colors (required)

Three color scales control the app chrome. Each uses a 50–950 numeric scale (50 = lightest, 950 = darkest).

| Token                      | Usage                             |
| -------------------------- | --------------------------------- |
| `--color-surface-{50-950}` | Backgrounds, borders, muted text  |
| `--color-primary-{50-950}` | Buttons, links, active states     |
| `--color-error-{50-950}`   | Destructive actions, error states |
| `--color-warning-{50-500}` | Warnings, caution badges          |

```css
:root {
  --color-surface-50: #ffffff;
  --color-surface-100: #f6f8fa;
  /* ... through 950 */
  --color-primary-50: #ddf4ff;
  --color-primary-500: #0969da;
  /* ... */
}
```

### Radii (optional)

| Token                | Default    | Usage                   |
| -------------------- | ---------- | ----------------------- |
| `--radius-base`      | `0.375rem` | Buttons, inputs, badges |
| `--radius-container` | `0.75rem`  | Cards, dialogs, panels  |

### Terminal Colors (required)

The standard 16-color ANSI palette plus chrome colors:

| Token                       | Usage               |
| --------------------------- | ------------------- |
| `--terminal-background`     | Terminal background |
| `--terminal-foreground`     | Default text color  |
| `--terminal-cursor`         | Cursor color        |
| `--terminal-selection`      | Selection highlight |
| `--terminal-black`          | ANSI color 0        |
| `--terminal-red`            | ANSI color 1        |
| `--terminal-green`          | ANSI color 2        |
| `--terminal-yellow`         | ANSI color 3        |
| `--terminal-blue`           | ANSI color 4        |
| `--terminal-magenta`        | ANSI color 5        |
| `--terminal-cyan`           | ANSI color 6        |
| `--terminal-white`          | ANSI color 7        |
| `--terminal-bright-black`   | ANSI color 8        |
| `--terminal-bright-red`     | ANSI color 9        |
| `--terminal-bright-green`   | ANSI color 10       |
| `--terminal-bright-yellow`  | ANSI color 11       |
| `--terminal-bright-blue`    | ANSI color 12       |
| `--terminal-bright-magenta` | ANSI color 13       |
| `--terminal-bright-cyan`    | ANSI color 14       |
| `--terminal-bright-white`   | ANSI color 15       |

### Editor Colors (required)

Base editor chrome:

| Token                  | Usage                                               |
| ---------------------- | --------------------------------------------------- |
| `--editor-background`  | Editor background                                   |
| `--editor-foreground`  | Default text / fallback for undefined syntax tokens |
| `--editor-selection`   | Selection highlight                                 |
| `--editor-line-number` | Gutter line numbers                                 |
| `--editor-added`       | Added text in diffs                                 |
| `--editor-deleted`     | Deleted text in diffs                               |
| `--editor-added-bg`    | Background for added lines                          |
| `--editor-deleted-bg`  | Background for deleted lines                        |

### Syntax Highlighting (optional)

Syntax tokens are optional — any undefined token falls back to `--editor-foreground`. Define as many or as few as you want:

| Token                  | Highlights                                                |
| ---------------------- | --------------------------------------------------------- |
| `--editor-keyword`     | `if`, `return`, `const`, `import`, control flow           |
| `--editor-string`      | String and character literals                             |
| `--editor-comment`     | Comments (also rendered italic)                           |
| `--editor-number`      | Numeric literals                                          |
| `--editor-variable`    | Variable names                                            |
| `--editor-type`        | Type/class names                                          |
| `--editor-function`    | Function/method names (falls back to `--editor-variable`) |
| `--editor-property`    | Property/field names                                      |
| `--editor-operator`    | Operators (`+`, `-`, `=>`, etc.)                          |
| `--editor-punctuation` | Brackets, semicolons, commas                              |
| `--editor-meta`        | Annotations, preprocessor directives                      |

Additional tokens for full coverage (all fall back through the chain to `--editor-foreground`):

| Token                | Highlights                                                                  |
| -------------------- | --------------------------------------------------------------------------- |
| `--editor-atom`      | Atomic values like booleans (falls back to `--editor-keyword`)              |
| `--editor-bool`      | Boolean literals (falls back to `--editor-atom`)                            |
| `--editor-string2`   | Regex, escape sequences, template strings (falls back to `--editor-string`) |
| `--editor-variable2` | Special variables (falls back to `--editor-variable`)                       |
| `--editor-class`     | Class names (falls back to `--editor-type`)                                 |
| `--editor-namespace` | Namespace/module names (falls back to `--editor-type`)                      |
| `--editor-macro`     | Macro names (falls back to `--editor-function`)                             |
| `--editor-label`     | Label names                                                                 |
| `--editor-link`      | Links in markup (falls back to `--editor-string`)                           |
| `--editor-heading`   | Headings in markup (falls back to `--editor-keyword`)                       |
| `--editor-literal`   | Generic literals (falls back to `--editor-string`)                          |
| `--editor-invalid`   | Invalid/error tokens (defaults to red with underline)                       |

## Minimal Theme Example

A minimal theme only needs to define the required tokens. Here's a stripped-down dark-only theme:

```css
/* Minimal dark theme — only .dark block needed if you never use light mode */

:root {
  --color-surface-50: #ffffff;
  --color-surface-100: #f0f0f0;
  --color-surface-200: #e0e0e0;
  --color-surface-300: #c0c0c0;
  --color-surface-400: #a0a0a0;
  --color-surface-500: #808080;
  --color-surface-600: #606060;
  --color-surface-700: #404040;
  --color-surface-800: #202020;
  --color-surface-900: #101010;
  --color-surface-950: #080808;
  --color-primary-50: #e0e0e0;
  --color-primary-100: #c0c0c0;
  --color-primary-200: #a0a0a0;
  --color-primary-300: #808080;
  --color-primary-400: #606060;
  --color-primary-500: #101010;
  --color-primary-600: #080808;
  --color-primary-700: #050505;
  --color-primary-800: #030303;
  --color-primary-900: #020202;
  --color-primary-950: #000000;
  --color-error-50: #fff0f0;
  --color-error-100: #ffcccc;
  --color-error-200: #ff9999;
  --color-error-300: #ff6666;
  --color-error-400: #ff3333;
  --color-error-500: #cc0000;
  --color-error-600: #aa0000;
  --color-error-700: #880000;
  --color-error-800: #660000;
  --color-error-900: #440000;
  --color-error-950: #220000;
  --color-warning-50: #fffde0;
  --color-warning-100: #fff9b0;
  --color-warning-200: #fff080;
  --color-warning-300: #ffe050;
  --color-warning-400: #ffd020;
  --color-warning-500: #ccaa00;
  --terminal-background: #ffffff;
  --terminal-foreground: #000000;
  --terminal-cursor: #0000ff;
  --terminal-selection: #add8e6;
  --terminal-black: #000000;
  --terminal-red: #cc0000;
  --terminal-green: #00cc00;
  --terminal-yellow: #cccc00;
  --terminal-blue: #0000cc;
  --terminal-magenta: #cc00cc;
  --terminal-cyan: #00cccc;
  --terminal-white: #cccccc;
  --terminal-bright-black: #666666;
  --terminal-bright-red: #ff0000;
  --terminal-bright-green: #00ff00;
  --terminal-bright-yellow: #ffff00;
  --terminal-bright-blue: #0000ff;
  --terminal-bright-magenta: #ff00ff;
  --terminal-bright-cyan: #00ffff;
  --terminal-bright-white: #ffffff;
  --editor-background: #ffffff;
  --editor-foreground: #000000;
  --editor-selection: #add8e6;
  --editor-line-number: #999999;
  --editor-added: #00cc00;
  --editor-deleted: #cc0000;
  --editor-added-bg: #e0ffe0;
  --editor-deleted-bg: #ffe0e0;
}

.dark {
  --color-surface-50: #e0e0e0;
  --color-surface-100: #c0c0c0;
  --color-surface-200: #a0a0a0;
  --color-surface-300: #808080;
  --color-surface-400: #606060;
  --color-surface-500: #404040;
  --color-surface-600: #303030;
  --color-surface-700: #202020;
  --color-surface-800: #181818;
  --color-surface-900: #101010;
  --color-surface-950: #080808;
  --color-primary-50: #080808;
  --color-primary-100: #101010;
  --color-primary-200: #303030;
  --color-primary-300: #606060;
  --color-primary-400: #a0a0a0;
  --color-primary-500: #f0f0f0;
  --color-primary-600: #e8e8e8;
  --color-primary-700: #e0e0e0;
  --color-primary-800: #f0f0f0;
  --color-primary-900: #f8f8f8;
  --color-primary-950: #ffffff;
  --terminal-background: #101010;
  --terminal-foreground: #e0e0e0;
  --terminal-cursor: #58a6ff;
  --terminal-selection: #264f78;
  --terminal-black: #404040;
  --terminal-red: #ff6666;
  --terminal-green: #66ff66;
  --terminal-yellow: #ffff66;
  --terminal-blue: #6666ff;
  --terminal-magenta: #ff66ff;
  --terminal-cyan: #66ffff;
  --terminal-white: #cccccc;
  --terminal-bright-black: #666666;
  --terminal-bright-red: #ff9999;
  --terminal-bright-green: #99ff99;
  --terminal-bright-yellow: #ffff99;
  --terminal-bright-blue: #9999ff;
  --terminal-bright-magenta: #ff99ff;
  --terminal-bright-cyan: #99ffff;
  --terminal-bright-white: #ffffff;
  --editor-background: #101010;
  --editor-foreground: #e0e0e0;
  --editor-selection: #264f78;
  --editor-line-number: #666666;
  --editor-added: #66ff66;
  --editor-deleted: #ff6666;
  --editor-added-bg: #1a3d1a;
  --editor-deleted-bg: #3d1a1a;
  /* Syntax — only define what you want to customize */
  --editor-keyword: #ff6666;
  --editor-string: #99ccff;
  --editor-comment: #888888;
}
```

## Bundled Themes

planeai ships with these themes (scaffolded to `~/.config/planeai/themes/` on first launch):

| Name         | Description                          |
| ------------ | ------------------------------------ |
| `default`    | Neutral gray, GitHub-inspired syntax |
| `github`     | GitHub Light / GitHub Dark           |
| `one`        | Atom One Light / Atom One Dark       |
| `catppuccin` | Catppuccin Latte / Macchiato         |
| `dracula`    | Dracula light variant / Dracula      |

## Tips

- Use any CSS color format: hex, hsl, rgb, oklch, etc.
- The surface scale should have good contrast between adjacent steps (e.g., 700 for backgrounds, 50 for text in dark mode).
- Test both light and dark modes — switch with the appearance mode in preferences.
- You can derive terminal/editor colors from your surface/primary scales for cohesion.
- iTerm2-Color-Schemes is a good source for terminal palettes: https://iterm2colorschemes.com
