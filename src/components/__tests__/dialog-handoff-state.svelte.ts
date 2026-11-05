/** Reactive open-state for the New Tab selector -> New Session dialog handoff harness. */
export interface DialogHandoff {
  showNewTab: boolean;
  showSession: boolean;
}

export function createDialogHandoff(): DialogHandoff {
  const handoff = $state({ showNewTab: true, showSession: false });
  return handoff;
}
