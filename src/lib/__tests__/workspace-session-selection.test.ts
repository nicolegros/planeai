import { describe, expect, it } from "vitest";
/**
 * These assertions read App.svelte as text, which only pins wiring that cannot be
 * extracted from the component. Prefer a behavioural test: the decisions this file
 * used to cover now live in task-workspace-layout.svelte.ts, layout-tree.ts,
 * terminal-focus.ts and the orchestrator, each with real unit tests. Add here only as a last resort.
 */
import appSource from "../../App.svelte?raw";

describe("TaskWorkspace session selection", () => {
  it("never leaves a stale task workspace selected when the task cannot be resolved", () => {
    // Done tasks are filtered out of the listing, and another project's tasks may
    // not be loaded yet. Without the else branch, selecting such a session kept
    // the previous selection and the main pane stayed on the wrong task.
    expect(appSource).toMatch(
      /if \(project && task\) \{\s*selectedTaskWorkspace = \{ project, task \};\s*touchWorkspaceMru\(toTaskWorkspaceId\(project\.id, task\.key\)\);\s*\} else \{[\s\S]*?selectedTaskWorkspace = null;\s*\}/,
    );
  });

  it("does not let a terminal from the previous layout reclaim focus during selection", () => {
    // The guard now lives in the tested `isTerminalPaneFocused` predicate; see
    // src/lib/__tests__/terminal-focus.test.ts for its behaviour.
    expect(appSource).toMatch(
      /focused=\{isTerminalPaneFocused\(\{[\s\S]*?belongsToActiveSession: sessionId === activeSessionId,/,
    );
  });

  it("releases xterm DOM focus on both ownership change and focus arrival", () => {
    // Behaviour lives in releaseTerminalDomFocus (see terminal-focus.test.ts).
    // Only the two trigger points are pinned here: a reactive-only release misses
    // programmatic focus requests, which suppress the focusin that moves the zone.
    expect(appSource).toMatch(/releaseTerminalDomFocus\(terminalKeyboardOwnership\)/);
    expect(appSource).toMatch(/addEventListener\("focusin"/);
    expect(appSource).toMatch(/removeEventListener\("focusin"/);
  });

  it("applies an adopted session from the caller, not from inside the layout loader", () => {
    expect(appSource).toMatch(
      /await workspaceLayout\.show\([\s\S]*?if \(shown\.adoptedSessionId\) orchestrator\.selectSession\(shown\.adoptedSessionId\)/,
    );
  });

  it("treats the task workspace's first linked session as an arbitrary entry point", () => {
    // Otherwise it overrides the restored layout's remembered tab and returning
    // to a task always lands on its first session.
    expect(appSource).toMatch(
      /const linked = sessions\.find\([\s\S]*?selectWorkspaceSession\(linked\.id, \{ explicit: false \}\);/,
    );
    expect(appSource).toMatch(/orchestrator\.selectSession\(sessionId, \{ explicit:/);
  });

  it("requests focus for the restored active terminal after loading a workspace layout", () => {
    expect(appSource).toMatch(
      /await workspaceLayout\.show\([\s\S]*?if \(shown\.restored\) requestFocusedTerminalFocus\(\);/,
    );
    expect(appSource).toMatch(
      /function requestFocusedTerminalFocus\(\): void \{[\s\S]*?workspaceLayout\.focusedTab\(\)[\s\S]*?requestTerminalFocus\(tab\.ptyKey\)/,
    );
  });

  it("gives an active main-pane plugin a flex-grown contribution area", () => {
    expect(appSource).toMatch(
      /\{#if activePluginId\}\s*<div class="flex h-full flex-col bg-main">[\s\S]*?<div class="min-h-0 flex-1">\s*<PluginContributionHost/,
    );
  });

  it("gives a session panel a live focused-agent recipient resolver", () => {
    expect(appSource).toMatch(
      /<PluginContributionHost[\s\S]*?session=\{activeContribution\.placement === "session\.panel" \? activePluginSessionContext : undefined\}[\s\S]*?getFocusedAgentSession=\{\(\) => activePluginSessionContext\}/,
    );
  });

  it("routes titlebar targets by their destination placement", () => {
    expect(appSource).toMatch(
      /const titlebarContributions = \$derived\([\s\S]*?contribution\.placement === "titlebar"/,
    );
    expect(appSource).toMatch(
      /function openTitlebarPluginContribution\(pluginId: string, contributionId: string\): void \{[\s\S]*?contribution\?\.placement === "session\.panel"[\s\S]*?openPluginContributionModal\(pluginId, contributionId\);[\s\S]*?openPluginContribution\(pluginId, contributionId\);/,
    );
    expect(appSource).toMatch(
      /<Titlebar[\s\S]*?\{titlebarContributions\}[\s\S]*?titlebarSession=\{activePluginSessionContext\}[\s\S]*?onOpenTitlebarContribution=\{openTitlebarPluginContribution\}/,
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
      /\{#if modalPlugin && modalContribution && activePluginSessionContext\}[\s\S]*?<FormDialog[\s\S]*?title=\{modalContribution\.label\}[\s\S]*?preventEscapeClose=\{false\}[\s\S]*?<PluginContributionHost[\s\S]*?session=\{activePluginSessionContext\}[\s\S]*?getFocusedAgentSession=\{\(\) => activePluginSessionContext\}[\s\S]*?closeOnEscape=\{true\}/,
    );
  });
});

it("ignores a hidden terminal's focus event after a session switch", () => {
  expect(appSource).toMatch(
    /onFocused=\{\(event\) => \{\s*if \(event\.type === "focusin" && !isActiveInLeaf\) return;/,
  );
});

it("preserves the keyboard-selected terminal PTY after session synchronization", () => {
  expect(appSource).toMatch(
    /function preserveKeyboardSelectedTerminal[\s\S]*?selectTerminalTab\(entry\.ptyKey\)[\s\S]*?workspaceLayout\.focusTab\(entry\.ptyKey\)[\s\S]*?requestTerminalFocus\(entry\.ptyKey\)/,
  );
  expect(appSource).toMatch(
    /function cycleTab\(delta: number\)[\s\S]*?preserveKeyboardSelectedTerminal\(tab\)/,
  );
});

it("keeps keyboard focus on the clicked terminal tab, not its session's agent", () => {
  // Selecting the session of a clicked shell used to focus the agent's terminal,
  // so a (agent | shell) split could never focus the shell.
  expect(appSource).toMatch(/requestTerminalFocus\(opts\.focusPtyKey \?\? sessionId\)/);
  expect(appSource).toMatch(
    /function selectTerminalTab\(ptyKey: string\)[\s\S]*?selectWorkspaceSession\(ptyKeySessionId\(ptyKey\), \{ focusPtyKey: ptyKey \}\)/,
  );
});

it("ignores split shortcuts while the layout is hidden or empty", () => {
  // A loop dashboard, plugin page or empty TaskWorkspace covers the layout;
  // splitting it there would act on panes the user cannot see.
  expect(appSource).toMatch(
    /const canSplit = \$derived\([\s\S]*?!isEmptyTaskWorkspace && !activeLoopId && !activePluginId/,
  );
  expect(appSource).toMatch(
    /function handleSplitAction\(actionType: string\): void \{\s*if \(!canSplit\) return;/,
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

it("parks an active agent session when Cmd+W closes its agent tab", () => {
  expect(appSource).toMatch(
    /outcome === "agent"\) \{[\s\S]*?await orchestrator\.parkSession\(session\);/,
  );
});
