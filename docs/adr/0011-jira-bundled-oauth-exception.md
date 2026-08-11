# ADR-0011: Bundled Jira OAuth credential exception

## Status

Accepted — 2026-08-11

## Context

PlaneAI's Jira Cloud integration uses Atlassian OAuth 2.0 three-legged OAuth (3LO). The desktop application already uses PKCE and a loopback callback, but Atlassian's 3LO registration also requires a client secret during authorization-code exchange and refresh-token rotation.

A desktop bundle cannot keep a client secret confidential: users can inspect the application or bundled plugin bytes. A public PKCE-only registration would avoid that distribution risk, but is not available for PlaneAI's Atlassian 3LO application. Operating an OAuth broker would keep the credential confidential but would add a PlaneAI-hosted security-sensitive service and operational dependency.

## Decision

Jira is an explicit exception for the bundled plugin:

- Bundle the Atlassian 3LO client ID and client secret with the desktop application. Treat the client secret as extractable, not confidential.
- Keep direct browser authorization using PKCE, the exact redirect URI `http://localhost:19287/callback`, and only `read:jira-work`, `write:jira-work`, and `offline_access` scopes.
- PlaneAI release engineering owns the Atlassian registration and credential rotation. Protected GitHub Actions `JIRA_CLIENT_ID` and `JIRA_CLIENT_SECRET` secrets inject the values into release builds; developers provide local values only through their uncommitted environment.
- Rotation requires updating the Atlassian registration, changing the protected release secret, and publishing a new PlaneAI release. It cannot revoke the extractable credential in already-installed releases.
- Preserve Jira settings, sources, cached issues, and task links through upgrades and reconnects. If Atlassian rejects a refresh token, clear only the stored refresh token and cloud ID, stop sync, and show the user a reconnect-required state.

## Consequences

The direct integration keeps its existing authorization, callback/state validation, refresh, cloud-ID resolution, connect/disconnect, sync, and writeback behavior without requiring a PlaneAI service.

The embedded credential must never be considered a security boundary. The Atlassian registration remains restricted to the stated redirect URI and minimal scopes, and release engineering must rotate it if compromise or operational need warrants it. A future Atlassian-supported public client or an approved broker can supersede this exception with a new ADR.
