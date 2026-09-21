import { describe, expect, it } from "vitest";
import appSource from "../../App.svelte?raw";

describe("TaskWorkspace session selection", () => {
  it("keeps a task workspace loaded and focuses a newly selected agent tab", () => {
    expect(appSource).toMatch(
      /if \(workspace\.key === lastTreeWorkspace\?\.key\) \{\s*if \(!focusedSessionChanged\) return;\s*lastFocusedWorkspaceSessionId = activeSessionId;\s*const existing = splitTree\.findTab\(activeSessionId\);/,
    );
  });

  it("builds a flat task workspace from every session linked to the same task", () => {
    expect(appSource).toMatch(
      /return sessions\.filter\(\(session\) => session\.project_id === workspace\.projectId && session\.task_key === workspace\.taskKey\);/,
    );
    expect(appSource).toMatch(/function buildTabEntriesForWorkspace\(workspace: WorkspaceIdentity\)/);
  });

  it("does not rewrite split-tree state when the focused task agent is already active", () => {
    expect(appSource).toMatch(
      /const focusedSessionChanged = activeSessionId !== lastFocusedWorkspaceSessionId;[\s\S]*?if \(!focusedSessionChanged\) return;/,
    );
    expect(appSource).toMatch(
      /if \(existing\.leaf\.activeTab !== activeSessionId\) splitTree\.focusTab\(activeSessionId\);/,
    );
  });

  it("does not let a terminal from the previous layout reclaim focus during selection", () => {
    expect(appSource).toMatch(
      /focused=\{isActiveInLeaf && sessionId === activeSessionId && !activePluginId/,
    );
  });

  it("adds restored task-session tabs without replacing the workspace layout", () => {
    expect(appSource).toMatch(
      /const missingEntries = workspaceEntries\.filter\(\(entry\) => !existingKeys\.has\(entry\.ptyKey\)\);[\s\S]*?for \(const entry of missingEntries\) splitTree\.addSessionToLeaf\(focusedLeaf\.id, entry\);[\s\S]*?splitTree\.setLeafActiveTab\(focusedLeaf\.id, activeTab\);/,
    );
  });

  it("gives an active main-pane plugin a flex-grown contribution area", () => {
    expect(appSource).toMatch(
      /\{#if activePluginId\}\s*<div class="flex h-full flex-col bg-main">[\s\S]*?<div class="min-h-0 flex-1">\s*<PluginContributionHost/,
    );
  });

  it("derives running titlebar contributions independently from session-panel commands", () => {
    expect(appSource).toMatch(
      /const titlebarContributions = \$derived\([\s\S]*?contribution\.placement === "titlebar"/,
    );
    expect(appSource).toMatch(
      /<Titlebar[\s\S]*?\{titlebarContributions\}[\s\S]*?titlebarSession=\{activePluginSessionContext\}[\s\S]*?onOpenTitlebarContribution=\{openPluginContributionModal\}/,
    );
  });

  it("routes an active session-panel plugin shortcut without legacy PR state", () => {
    expect(appSource).toMatch(
      /findPluginShortcut\(event, sessionPanelCommands, mainPaneCommands\)/,
    );
    expect(appSource).toMatch(/window\.addEventListener\("keydown", onPluginShortcut, true\)/);
    expect(appSource).toMatch(
      /target\.contribution\.placement === "session\.panel"[\s\S]*?openPluginContributionModal\(target\.plugin\.id, target\.contribution\.id\)/,
    );
    expect(appSource).not.toContain("showPrPanel");
  });

  it("opens only a titlebar navigation target session panel in the generic modal", () => {
    expect(appSource).toMatch(
      /function openPluginContributionModal\(pluginId: string, contributionId: string\): void \{[\s\S]*?candidate\.placement === "session\.panel"[\s\S]*?modalPluginId = pluginId;/,
    );
    expect(appSource).toMatch(
      /\{#if modalPlugin && modalContribution && activePluginSessionContext\}[\s\S]*?<FormDialog[\s\S]*?title=\{modalContribution\.label\}[\s\S]*?preventEscapeClose=\{false\}[\s\S]*?<PluginContributionHost[\s\S]*?session=\{activePluginSessionContext\}[\s\S]*?closeOnEscape=\{true\}/,
    );
  });
});

it("ignores a previous terminal's focus event after a session switch", () => {
  expect(appSource).toMatch(
    /onFocused=\{\(event\) => \{\s*if \(event\.type === "focusin" && sessionId !== activeSessionId\) return;/,
  );
});

it("preserves editor focus when a split-pane click originates inside an editor", () => {
  expect(appSource).toMatch(
    /event\.target instanceof Element && event\.target\.closest\("\[data-editor-tab\]"\)[\s\S]*?focusTerminal\(\);/,
  );
});

it("allows a session-panel modal to use its contribution's reported content height", () => {
  expect(appSource).not.toContain('class="h-[min(78vh,720px)]"');
  expect(appSource).toMatch(
    /\{#if modalPlugin && modalContribution && activePluginSessionContext\}[\s\S]*?<FormDialog[\s\S]*?title=\{modalContribution\.label\}[\s\S]*?class="min-h-\[min\(360px,85vh\)\]"[\s\S]*?preventEscapeClose=\{false\}[\s\S]*?<div>\s*<PluginContributionHost/,
  );
});

it("reserves initial modal focus for the session-panel plugin iframe", () => {
  expect(appSource).toMatch(
    /\{#if modalPlugin && modalContribution && activePluginSessionContext\}[\s\S]*?<FormDialog[\s\S]*?preventOpenAutoFocus=\{true\}[\s\S]*?<PluginContributionHost[\s\S]*?autofocus=\{true\}/,
  );
});

it("does not expose a direct legacy PR API", async () => {
  const apiSource = await import("../api.ts?raw").then((module) => module.default);
  expect(apiSource).not.toMatch(/export const pr\s*=/);
  expect(apiSource).not.toContain('"fetch_pr_url"');
  expect(apiSource).not.toContain('"create_pr"');
  expect(apiSource).not.toContain('"merge_pr"');
});

it("does not re-close a shell tab after its explicit close removed it from the split tree", () => {
  expect(appSource).toMatch(
    /const unlistenShellPtyExit = listen<[\s\S]*?if \(!splitTree\.findTab\(ptyKey\)\) return;[\s\S]*?orchestrator\.closeShellTab\(sessionId, tabIndex\)\.catch\(/,
  );
});

it("awaits shell-tab closure before removing the split-tree entry and contains failures", () => {
  expect(appSource).toMatch(
    /async function closeShellTabInTree[\s\S]*?await orchestrator\.closeShellTab\(sessionId, tabIndex\);[\s\S]*?splitTree\.removeSessionFromLeaf\(ptyKey\);/,
  );
  expect(appSource).toMatch(
    /async function closeShellTabInTree[\s\S]*?try \{[\s\S]*?await orchestrator\.closeShellTab[\s\S]*?\} catch \(error\) \{[\s\S]*?showSnackbar\(/,
  );
});

it("archives an active agent session when Cmd+W closes its agent tab", () => {
  expect(appSource).toMatch(
    /if \(activeEntry\.type === "agent"\) \{[\s\S]*?await orchestrator\.archiveSession\(session\);/,
  );
});
