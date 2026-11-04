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
- **Timeouts.** `timeoutMs` bounds a call, from 1 to 120000 milliseconds, 30000 by default.

## Consequences

- `planeai-cli plugin test` and installation reject the same manifests, since both validate through `planeai-plugin-contract`; installation also checks the served names against PlaneAI's tool table.
- A plugin keeps declaring tools in its manifest; PlaneAI never learns tools from the sidecar at runtime, so the inventory knows a plugin's tools before it starts.
