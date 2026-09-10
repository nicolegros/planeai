import { describe, expect, it } from "vitest";
import appSource from "../../App.svelte?raw";

describe("workspace session selection", () => {
  it("activates an already-loaded session tab when the active session changes", () => {
    expect(appSource).toMatch(
      /if \(hasActive\) \{\s*splitTree\.focusTab\(activeSessionId\);\s*lastTreeSessionId = activeSessionId;/,
    );
  });

  it("only treats the primary agent tab as an already-loaded session terminal", () => {
    expect(appSource).toMatch(
      /const hasActive = allLeaves\.some\(\(leaf\) =>\s*leaf\.tabs\.some\(\(t\) => t\.ptyKey === activeSessionId\)\s*\);/,
    );
  });

  it("does not let a terminal from the previous layout reclaim focus during selection", () => {
    expect(appSource).toMatch(
      /focused=\{isActiveInLeaf && sessionId === activeSessionId && !activePluginId/,
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

  it("routes an active session-panel plugin shortcut before the legacy PR fallback", () => {
    expect(appSource).toMatch(
      /findPluginShortcut\(event, sessionPanelCommands, mainPaneCommands\)/,
    );
    expect(appSource).toMatch(
      /window\.addEventListener\("keydown", onPluginShortcut, true\)/,
    );
    expect(appSource).toMatch(
      /target\.contribution\.placement === "session\.panel"[\s\S]*?showPrPanel = false;[\s\S]*?openPluginContributionModal\(target\.plugin\.id, target\.contribution\.id\)/,
    );
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
