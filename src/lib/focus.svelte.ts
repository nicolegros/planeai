export type FocusZone = "terminal" | "sidebar" | "editor" | "explorer" | "none";
export type SidebarSubZone = "sessions" | "tasks";

type ExplorerReturnZone = "terminal" | "editor";

let activeZone = $state<FocusZone>("terminal");
let explorerReturnZone = $state<ExplorerReturnZone>("terminal");
let sidebarSubZone = $state<SidebarSubZone>("sessions");

export function getActiveZone(): FocusZone {
  return activeZone;
}

export function getExplorerReturnZone(): ExplorerReturnZone {
  return explorerReturnZone;
}

export function getSidebarSubZone(): SidebarSubZone {
  return sidebarSubZone;
}

export function setActiveZone(zone: FocusZone): void {
  activeZone = zone;
}

export function focusTerminal(): void {
  activeZone = "terminal";
}

/** Force terminal re-focus even if already the active zone */
export function refocusTerminal(): void {
  activeZone = "none";
  activeZone = "terminal";
}

export function focusSidebar(): void {
  activeZone = "sidebar";
}

export function focusEditor(): void {
  activeZone = "editor";
}

export function focusExplorer(): void {
  if (activeZone !== "explorer") {
    explorerReturnZone = activeZone === "editor" ? "editor" : "terminal";
  }
  activeZone = "explorer";
}

/** Toggle keyboard focus between Explorer and the zone that opened it. */
export function toggleExplorerFocus(): void {
  if (activeZone === "explorer") {
    activeZone = explorerReturnZone;
  } else {
    focusExplorer();
  }
}

export function toggleSidebar(): void {
  activeZone = activeZone === "sidebar" ? "terminal" : "sidebar";
}

export function toggleSessionsPanel(): void {
  if (activeZone === "sidebar" && sidebarSubZone === "sessions") {
    activeZone = "terminal";
  } else {
    activeZone = "sidebar";
    sidebarSubZone = "sessions";
  }
}

export function toggleTaskPanel(): void {
  if (activeZone === "sidebar" && sidebarSubZone === "tasks") {
    activeZone = "terminal";
  } else {
    activeZone = "sidebar";
    sidebarSubZone = "tasks";
  }
}
