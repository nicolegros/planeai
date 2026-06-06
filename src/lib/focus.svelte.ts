export type FocusZone = "terminal" | "sidebar" | "explorer";

let activeZone = $state<FocusZone>("terminal");

export function getActiveZone(): FocusZone {
  return activeZone;
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
