export type FocusZone = "terminal" | "sidebar" | "explorer";
export type SidebarSubZone = "sessions" | "tasks";

let activeZone = $state<FocusZone>("terminal");
let sidebarSubZone = $state<SidebarSubZone>("sessions");

export function getActiveZone(): FocusZone {
  return activeZone;
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

export function focusSidebar(): void {
  activeZone = "sidebar";
}

export function focusExplorer(): void {
  activeZone = "explorer";
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
