---
title: Plugin UI contributions
description: Add manifest-declared, isolated plugin UI to PlaneAI.
---

Plugins declare `ui_contributions` in `planeai-plugin.json`. Each contribution has a safe per-plugin `id`, user-facing `label`, `placement`, and self-contained ESM `entrypoint`. Supported placements are `sidebar.header`, `sidebar.navigation`, `sidebar.footer`, and `main-pane`. Sidebar contributions may use `order`; main panes may declare an optional portable `Mod+…` shortcut and are always available through Cmd+K while running.

An entrypoint default-exports `{ mount(root, context) { return dispose; } }`. `root` is an isolated ShadowRoot. Context provides plugin metadata, contribution metadata, scoped sidecar RPC through `host.call`, and `host.navigation.open(pluginId, contributionId)` / `host.navigation.close()`. Contributions may navigate across running plugins to declared main panes.

Use inherited PlaneAI CSS tokens such as `--color-t1`, `--color-panel`, and `--color-border`; do not depend on PlaneAI DOM or Tailwind classes. Bundles must be self-contained: relative imports and package asset graphs are not loaded in v1. Rebuild and reinstall an immutable local package to pick up manifest or UI changes.
