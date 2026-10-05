# ADR-0015: Backend-owned Terminal tab lifecycle

## Status

Accepted

## Context

Closing one shell tab spanned two processes.
The layout held a pending command, the next shell index, the shells gone this run and the closes in flight; `PtyManager` held its own close claims (`Closing`/`Closed`) and which tabs ran a program.
Each side had to absorb the exit echo of every close, and a webview reload wiped the frontend half while the backend kept running, so a key could be handed out again and meet the previous tab's state.
Review passes on provider handoffs (ADR-0014) found nearly all their late bugs in this protocol: close-vs-spawn races, exit echoes, key reuse after a reload, adopting a dead program.

## Decision

- A backend **Terminal tabs** module owns the lifecycle of every tab the layout opens on a PTY: shells, terminal editors and provider handoff programs, on the local, daemon and rmux backends.
  Agent tabs stay with the session lifecycle; diff and editor tabs stay layout-only.
- Each tab goes Reserved → Spawning → Live → Closing → Gone.
  `open(session, spec)` reserves a tab and returns its id, and `attach(tab, channel)` spawns it, or reconnects to a live one without ever starting another process.
  A failed reconnect leaves the tab live while its process runs, and ends it as `exited` otherwise.
  `close(tab)` fails only when the kill fails, leaving the tab live.
  A tab that cannot start ends as `failed_to_start`, its error carried by its end rather than by the attach, unless its process may run on in a daemon or rmux the host could not reach, when it stays to be closed or started again.
- The backend hands out shell indices from a per-session counter persisted in the database, so a pty key is never reused, across reloads or restarts.
  This reverses "the layout is the authority; there is no per-session tab count".
  The pty key grammar does not change.
- Tabs that ended are persisted, so restoring a layout prunes them exactly, even one saved while its workspace was not shown.
  Local shells that died with the app are not ended: their tabs spawn a fresh shell, as before.
- A tab leaves the layout only through one `TabEnded { reason }` event (closed, exited, failed to start, session ended), emitted exactly once per tab whoever caused it.
- With keys never reused, every tab follows the same rules: closing a reserved tab ends it at once, a close during a spawn waits for it and ends what it started, and attaching to an ended tab only reports that it ended (`attach_tab` returns false), its one `TabEnded` remaining the report of its end.
  This replaces the shell-only reattach after close and the "cannot close a terminal editor until it attached" rule.
- `PtyManager` keeps only the byte transport (write, resize, flow control) shared with agent PTYs, and reaches each backend through one adapter per backend.

## Consequences

The layout sends intents and renders `TabEnded`; it no longer allocates indices or tracks closes.
The reserved spec lives in memory only, so a tab reserved but not yet spawned when the app quits comes back as a plain shell, as a handoff tab already does.
