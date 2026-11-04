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
- A launch inside the app reads the server's live state: while it listens the agent gets `mcp_args` and a token, and while it is off or failed it gets neither, so it never connects to a port the app could not bind.
  A launch from `planeai-cli` uses the configured URL (`http://127.0.0.1:<mcp_port>/mcp` while `mcp_enabled`); when the app is not running, the agent's MCP connection fails and it runs without PlaneAI tools.
  The server starts before startup revives tmux sessions, so revived agents reconnect.
- The threat model is one trusted user on their own machine.
  The bearer token tells sessions apart and keeps out browsers and other local apps that do not have the agent's environment.
  Other OS users on the same machine and a process squatting the port while the app is closed are out of scope.
- Every agent launch mints a bearer token for its session.
  Only its SHA-256 hash is stored, in the `mcp_tokens` table, so tokens survive app restarts without the database holding a usable credential.
  Triggers revoke the token when the session is archived, destroyed or deleted.
  A request without a valid token gets 401, and the token identifies the calling session, its project and its task.
- The server rejects any `Host` that is not loopback and any request carrying a browser `Origin`, which blocks DNS rebinding from web pages.
- The agent gets `PLANEAI_MCP_URL` and `PLANEAI_MCP_TOKEN` in its environment.
  Each provider has an `mcp_args` template appended to its launch command, where `{mcp_url}` becomes the server URL.
  The token never appears on a command line: Claude Code expands `${PLANEAI_MCP_TOKEN}` in its `--mcp-config` header, and Codex reads it through `bearer_token_env_var`.
  tmux can only set an environment variable through `new-session -e`, which shows in `ps` and stays in the tmux session's environment, so a tmux launch writes the token to a private file in the app data folder instead; the agent's shell reads it, deletes it and exports it before running the agent.
  An empty `mcp_args` opts a provider out.
- Claude Code and Codex ship with templates.
  Kiro and Copilot ship empty, because neither has a verified per-launch way to add an MCP server; users can fill in their own.
- Every tool carries MCP annotations derived from its effect: read, write (idempotent or not) or delete.
  The templates pre-approve reads, creates and updates, and leave `task_delete` to prompt.
  They do not list tools: `{mcp_allowed_tools}` (Claude Code's `mcp__planeai__<tool>` names) and `{mcp_codex_prompted_tools}` (Codex's per-tool `approval_mode="prompt"` overrides) are filled at launch from the tool table in `planeai-core`, which the app's registry must match, as a test checks.
- The tools are `task_list`, `task_show`, `task_create`, `task_update`, `task_delete`, `session_list`, `session_tree`, `session_read`, `session_create`, `session_prompt` and `project_list`.
  They default to the caller: `task_show` and `task_update` without a key act on the session's task, task tools work in the project of that task, `session_tree` starts from the calling session, and `session_create` makes a child of it.
- A session never reaches more permissions through the tools than it has.
  `session_create` defaults `yolo` to the caller's own setting and refuses `yolo: true` from a session that asks before running tools, and `session_prompt` refuses to prompt a yolo session from such a session.
  The check runs in the server, so it holds although the templates pre-approve both tools.
- `task_update` checks its status, parent and blockers before writing, so a refused update changes nothing.
  The task store refuses a parent that is missing, the task itself or one of its subtasks, drops repeated tags and blockers, and writes each field edit in one transaction.
  The field edit, the parent change and the status move are three transactions: the parent change and the move report lifecycle events through task provider methods that work on any provider.
  Every call of a task-writing tool refreshes the GUI once, from the tool registry, even a failed one.
- Tools are thin adapters over the AXI domain functions and return the same TOON text the CLI prints, without its CLI-specific help hints.
  `planeai-cli axi` stays for agents outside PlaneAI sessions; there is one implementation behind both.
  A tool call opens one database connection and never migrates: the app migrated at startup. Token lookups share one connection for the server's lifetime.
- Tools live in a registry of definitions (name, description, effect, input schema, handler), so plugin tools can join them through plugin host API v6 (ADR-0017).

## Consequences

- An agent started while the server is off, or before this change, has no PlaneAI tools until it is restarted.
- Changing the port strands running agents on the old URL until they restart; Settings says so.
- The skills that wrap `planeai-cli axi` become redundant inside sessions and can be removed once plugin tools and provider runtimes reach MCP.
- Adding a tool means a registry entry with typed input; its annotations, and therefore its approval in the templates, follow from its effect.
- Accepted risk: a session that asks before running tools can still create a task without asking, since `task_create` is pre-approved.
  In a project with auto-dispatch on, that task is then picked up and run with yolo.
  This is accepted because auto-dispatch is opt-in per project, and enabling it already means trusting every todo task in it, whoever wrote it.
