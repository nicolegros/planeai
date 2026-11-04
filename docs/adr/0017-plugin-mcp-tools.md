# ADR-0017: Plugins contribute MCP tools

## Status

Accepted

## Context

ADR-0016 gave agents in PlaneAI sessions an in-app MCP server whose tools act as the calling session, and planned plugin tools through plugin host API v6.
Integrations such as Jira and GitHub live in trusted local plugins (ADR-0011), so the tools agents want from them (show an issue, link a pull request) belong to those plugins.
Agents should call them like PlaneAI's own tools: same server, same token, same approval rules, without a second MCP server per plugin.

## Decision

- **Contract.** A manifest on `planeai.plugin-host.v6` may declare `tools: [{ name, description, inputSchema, annotations?, timeoutMs? }]` together with the `tools` capability; either one without the other is rejected.
  Bundled plugins cannot declare tools.
  Unknown tool and annotation fields are rejected, like the rest of the manifest.
- **Names.** A tool is served as `<plugin id>_<name>`.
  `name` is lowercase letters, digits and underscores, starting with a letter, unique within the plugin.
  Plugin ids contain no underscore, so the plugin part of a served name is unambiguous and two plugins never collide.
  A served name that is one of PlaneAI's own tools, or longer than 50 characters (Claude Code and Codex cap `mcp__planeai__<name>` at 64), is refused when the package installs.
- **Effects.** `annotations` carries MCP's hints with MCP's defaults, and they map onto ADR-0016's effects: `readOnlyHint` is a read, a tool that is not read-only is a delete unless `destructiveHint` is `false`, and then a write, idempotent per `idempotentHint`.
  A tool without annotations is therefore destructive, so a careless declaration errs toward asking.
  Read-only and destructive together is rejected.
- **Serving.** The app's tool registry is the one list the server serves: PlaneAI's own tools, then each running plugin's set by plugin id.
  The plugin supervisor publishes a plugin's set after its handshake, before the plugin shows as running, and retires it on any move away from running (disable, reload, a failed request that retires the runtime, an exited process).
  A set holding a name already served is refused whole and the plugin fails to start with that error; installation catches this first, so it only happens when a newer PlaneAI adds a tool a plugin already uses.
  Every call, PlaneAI's or a plugin's, goes through the same registry lookup and `ToolDef::call`; a handler is a future, so a plugin call waits on the sidecar without holding a thread.
- **Listing changes.** Enabling or disabling a plugin changes `tools/list` without restarting the app.
  The server stays in stateless JSON mode (ADR-0016), where there is no session to send `notifications/tools/list_changed` to, so it does not advertise `listChanged`; clients see a change on their next list, which Claude Code and Codex make when they connect.
- **Call.** The host sends `tool.call` with `{ tool, arguments, caller }`.
  `caller` is the session from the bearer token: `session_id`, `project` (`id`, `name`), `task` (`key` and its owning `project`, or `null`) and `auto_approve`.
  The plugin answers `{ text, is_error? }`, the shape PlaneAI's own tools return, and the server wraps it as one MCP text block.
  An invalid result, a JSON-RPC error, a plugin that is not running and a timeout all reach the agent as an error result, never as a protocol failure.
  `tool.call` is host-controlled like `provider.*`, so plugin UI cannot forge a caller.
- **Timeouts.** `timeoutMs` bounds a call, from 1 to 120000 milliseconds, 30000 by default.
  On expiry the host sends `$/cancelRequest` (ADR-0011); a plugin that acknowledges keeps running and the agent reads that the call was cancelled, one that does not is stopped.
  The HTTP request ends with the call either way, and the sidecar request lane is shared, so a slow tool delays the plugin's other requests.
- **Approvals.** Plugin tools follow ADR-0016's effect rule: reads and writes run without asking, deletes ask.
  A launch from the app fills `{mcp_allowed_tools}` and `{mcp_codex_approved_tools}` with PlaneAI's approved tools plus the read and write tools of the plugins running at that moment, read from the registry the server lists.
  This is the simplest option that keeps both properties: Claude Code and Codex list tools when they connect, so what an agent sees is what was running at its launch, and its approvals match.
  Codex's template moves from approving by default with `prompt` overrides to asking by default with `approve` overrides, so on both agents a tool missing from the launch list, a delete or one from a plugin enabled later, asks.
  The placeholder is renamed from `{mcp_codex_prompted_tools}` to `{mcp_codex_approved_tools}` to match.
  v1.113 to v1.114 shipped the old template, and a saved config may hold it: loading the config upgrades it (the shipped template becomes the current one, and a custom one has its `approve` default and old placeholder replaced), and the launch applies the same upgrade, so the old placeholder never reaches an agent's arguments.
  Sessions started through `planeai-cli` or the `session_create` tool, which do not reach the registry, read the same set from the plugin inventory: the declared tools of plugins whose row says `running`, which the supervisor writes right after publishing and right before retiring.
  While the app is closed no row says `running`, and such agents cannot connect anyway.
  A session revived at app startup gets PlaneAI's tools only, as plugins start after revival.
  Pre-approving plugin tools by declared effect trusts the plugin's annotations, which is consistent with plugins being trusted code.
  Rejected: leaving every plugin tool asking, which makes a read-only plugin tool cost a prompt per call and breaks non-interactive agents.
- **Escalation.** ADR-0016's guard stays in PlaneAI's tools.
  A plugin receives `auto_approve` so it can apply the same rule to what its tools start; plugins are trusted code (ADR-0011), so the host does not enforce it for them.

## Consequences

- `planeai-cli plugin test` and installation reject the same manifests, since both validate through `planeai-plugin-contract`; installation also checks the served names against PlaneAI's tool table.
- `planeai-cli plugin test` sends `tool.call` for scenario lines `{ tool, arguments }` and checks the result shape; the local fixture declares a read tool and a destructive one and `make test-plugin-fixture` calls both.
- A plugin keeps declaring tools in its manifest; PlaneAI never learns tools from the sidecar at runtime, so the inventory knows a plugin's tools before it starts.
