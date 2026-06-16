# Dual Session Backend — Development Plan

> **Note:** This plan is superseded by ADR-0010. The `direct` backend described here has been replaced by the `daemon` backend, which provides session persistence without requiring tmux. See PLA-65/PLA-70 for the implementation.

## Phase 1: Backend foundation (Rust)

### 1.1 DB migration — add `backend` column and `exited` status

- Add migration in `db.rs`: `ALTER TABLE sessions ADD COLUMN backend TEXT NOT NULL DEFAULT 'tmux' CHECK(backend IN ('tmux', 'direct'))`
- Update `status` CHECK constraint to include `'exited'`
- Update `Session` struct to include `backend: String`
- Add `mark_session_exited(conn, id)` helper

### 1.2 Config — add `session_backend` field

- In `config.rs`, add `session_backend: Option<String>` to `Config` struct (serde skip_serializing_if none)
- Add `resolve_backend(config) -> &str` function: checks config field, falls back to tmux-on-PATH detection
- Add `tmux_available() -> bool` helper (check PATH for tmux binary)

### 1.3 PtyManager — `PtyTarget` enum and unified attach

- Define `PtyTarget` enum in `pty.rs`:
  ```rust
  pub enum PtyTarget {
      TmuxAttach { tmux_name: String },
      Direct { command: String, args: Vec<String>, cwd: String },
  }
  ```
- Refactor `attach()` to accept `PtyTarget` instead of `tmux_name: &str`
- For `Direct`: build `CommandBuilder` with command/args/cwd
- For `TmuxAttach`: existing logic (spawn `tmux attach-session`)
- Both paths share the reader thread, event emission, and notify integration

### 1.4 Exit detection — emit `pty-exited` event

- When PTY reader thread gets EOF (read returns 0), emit `pty-exited-{session_id}` event to frontend
- Add Tauri command `mark_exited(session_id)` that updates DB status to `'exited'`
- Remove `remain-on-exit` from `tmux::create_session_with_cmd()`

### 1.5 Startup reconciliation

- In `setup()`, after DB open and before managing state:
  - Query all sessions with `status = 'active'`
  - For `backend = 'direct'` → mark exited (process died with app)
  - For `backend = 'tmux'` → check `tmux::has_session()`; if false → mark exited
- Remove the current "destroy orphan sessions" logic in `list_sessions` (replace with reconciliation)

## Phase 2: Launch flow changes (Rust)

### 2.1 Refactor `launch_session` command

- Read resolved backend from config state
- Branch on backend:
  - **tmux**: existing flow (create tmux session, store tmux_name)
  - **direct**: skip tmux creation, store `backend = 'direct'`, `tmux_name = NULL`
- Pass `backend` to `db::create_session_with_id()`

### 2.2 Refactor `attach_session` command

- Change signature: accept `session_id`, `backend`, `tmux_name` (optional), `command`/`args`/`cwd` (optional)
- Build `PtyTarget` from params and call `pty_manager.attach()`
- Or simpler: two commands `attach_tmux_session` and `attach_direct_session` (avoids complex optional params)
  - **Decision**: single command with a JSON payload that maps to `PtyTarget`

### 2.3 Refactor `destroy_session` command

- Only call `tmux::kill_session()` if `backend = 'tmux'` and `tmux_name` is present
- Direct sessions: just detach PTY and remove DB row

### 2.4 Add `restart_session` command

- Takes `session_id`
- Reads session from DB (get backend, project, working dir, provider, auto_approve)
- If tmux: create new tmux session with same name, attach
- If direct: attach with `PtyTarget::Direct`
- Update status back to `'active'`

## Phase 3: Frontend changes (Svelte)

### 3.1 Listen for `pty-exited-{session_id}` events

- In `App.svelte` or a session manager module, listen for exit events
- Update local session state to `exited`
- Call `mark_exited` Tauri command to persist

### 3.2 Sidebar — exited state indicator

- Show "exited" badge/dimmed style on exited sessions
- Add "Restart" action (button or context menu) for exited sessions
- Backend type in tooltip on hover

### 3.3 Quit confirmation modal

- Listen for Tauri `close-requested` event (via `onCloseRequested`)
- Prevent default
- Check: any sessions with `status = 'active'` and `backend = 'direct'`?
  - No → `appWindow.destroy()`
  - Yes → show confirmation modal with count
- On confirm → `appWindow.destroy()`
- On cancel → dismiss

### 3.4 Preferences — session backend dropdown

- Add to `PreferencesPage.svelte`:
  - Dropdown: Auto / tmux / Direct
  - Map to config: Auto = remove field, tmux/direct = set field
  - Inline warning if "tmux" selected but `tmux_available()` returns false
- Add Tauri command `check_tmux_available() -> bool`

### 3.5 SessionForm — no changes needed

- Backend is global, not per-session. Form stays the same.

### 3.6 Terminal component — handle exited state

- When session is exited: disable input (don't send keystrokes to PTY)
- Show a restart prompt/button overlay or in the toolbar

## Phase 4: Polish

### 4.1 Auto-detect on first launch

- If config has no `session_backend` field, resolve at startup
- Surface the resolved backend somewhere visible (e.g., Preferences shows "Auto (using tmux)" or "Auto (using direct)")

### 4.2 Edge cases

- User force-quits (SIGKILL) with direct sessions → handled by startup reconciliation
- tmux server crashes mid-session → PTY reader gets EOF → normal exit flow
- User switches backend in Preferences → existing sessions unaffected (already tested by design)

### 4.3 Tests

- Rust unit tests for `resolve_backend()` logic
- Rust unit tests for `PtyTarget` construction
- Frontend: test quit confirmation logic (mock session list)

## Suggested implementation order

1. **1.1** (DB migration) — foundation for everything
2. **1.2** (Config) — needed by launch flow
3. **1.3** (PtyTarget enum) — core abstraction
4. **1.4** (Exit detection) — enables exited state
5. **2.1** (Launch refactor) — creates direct sessions
6. **2.2** (Attach refactor) — connects direct sessions to PTY
7. **1.5** (Startup reconciliation) — handles restart
8. **2.3** (Destroy refactor) — cleanup for both backends
9. **3.1 + 3.2** (Frontend exit handling + sidebar)
10. **3.3** (Quit confirmation)
11. **3.4** (Preferences dropdown)
12. **2.4 + 3.6** (Restart)
13. **4.x** (Polish)
