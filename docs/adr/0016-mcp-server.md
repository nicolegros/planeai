# ADR-0016: In-app MCP server for agents in sessions

## Status

Accepted

## Context

Agents running in PlaneAI sessions manage tasks and other sessions through skills that shell out to `planeai-cli axi`.
Every call costs a shell round trip, a prompt the user often has to approve, and instructions the skill has to carry about flags and output.
The agent also has to work out which task and session it is running in, which the CLI can only guess from the working directory.
Claude Code and Codex both speak MCP natively, show its tools by name, and let a launch pre-approve them.

## Decision

- The app serves MCP over streamable HTTP at `http://127.0.0.1:<port>/mcp`, in stateless JSON mode.
  A stdio server was rejected: it would be one more process per agent, each opening the database and reaching the app over the notify socket, while tool calls inside the app share its process and refresh the GUI directly.
- The port is stable and configurable (`mcp_port`, default 47437), and `mcp_enabled` turns the server off.
  Agents outlive app restarts and keep the URL they were launched with, so the server never moves to another port.
  A taken port is reported as a failed status in **Preferences → Agents → PlaneAI tools**, and the next config save or reload retries it.
- A launch connects its agent only while the server is listening: a launch inside the app reads the server's live state, and while it is off or failed the agent gets neither `mcp_args` nor a token, so it never trusts a port another process may hold.
  The CLI runs outside the app, so the app publishes `mcp-server.json` (its pid and port, mode 0600) in the app data folder while it listens, and removes it when the server stops, fails or the app exits.
  The CLI connects its agents only when that file names a live process; a file left by a crashed app names a dead pid and is ignored.
  A file was chosen over asking the app on the notify socket: it needs no protocol, and the pid check makes it safe when stale.
  The server starts before startup revives tmux sessions, so revived agents reconnect.
- Every agent launch mints a bearer token for its session.
  Only its SHA-256 hash is stored, in the `mcp_tokens` table, so tokens survive app restarts without the database holding a usable credential.
  Triggers revoke the token when the session is archived, destroyed or deleted.
  A request without a valid token gets 401, and the token identifies the calling session, its project and its task.
- The server rejects any `Host` that is not loopback and any request carrying a browser `Origin`, which blocks DNS rebinding from web pages.
- The agent gets `PLANEAI_MCP_URL` and `PLANEAI_MCP_TOKEN` in its environment.
  Each provider has an `mcp_args` template appended to its launch command, where `{mcp_url}` becomes the server URL.
  The token never appears on a command line: Claude Code expands `${PLANEAI_MCP_TOKEN}` in its `--mcp-config` header, and Codex reads it through `bearer_token_env_var`.
  An empty `mcp_args` opts a provider out.
- Claude Code and Codex ship with templates.
  Kiro and Copilot ship empty, because neither has a verified per-launch way to add an MCP server; users can fill in their own.
- Every tool carries MCP annotations derived from its effect: read, write (idempotent or not) or delete.
  The templates pre-approve reads, creates and updates, and leave `task_delete` to prompt.
- The tools are `task_list`, `task_show`, `task_create`, `task_update`, `task_delete`, `session_list`, `session_tree`, `session_read`, `session_create`, `session_prompt` and `project_list`.
  They default to the caller: `task_show` and `task_update` without a key act on the session's task, task tools work in the project of that task, `session_tree` starts from the calling session, and `session_create` makes a child of it.
- A session never reaches more permissions through the tools than it has.
  `session_create` defaults `yolo` to the caller's own setting and refuses `yolo: true` from a session that asks before running tools, and `session_prompt` refuses to prompt a yolo session from such a session.
  The check runs in the server, so it holds although the templates pre-approve both tools.
- Tools are thin adapters over the AXI domain functions and return the same TOON text the CLI prints, without its CLI-specific help hints.
  `planeai-cli axi` stays for agents outside PlaneAI sessions; there is one implementation behind both.
- Tools live in a registry of definitions (name, description, effect, input schema, handler), so plugin tools can be appended later through plugin host API v6.

## Consequences

- An agent started while the server is off, or before this change, has no PlaneAI tools until it is restarted.
- Changing the port strands running agents on the old URL until they restart; Settings says so.
- The skills that wrap `planeai-cli axi` become redundant inside sessions and can be removed once plugin tools and provider runtimes reach MCP.
- Adding a tool means a registry entry with typed input; its annotations, and therefore its approval in the templates, follow from its effect.
