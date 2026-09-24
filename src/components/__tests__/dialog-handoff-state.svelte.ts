/** Reactive open-state for the New Item -> New Session dialog handoff harness. */
export interface DialogHandoff {
  showNewItem: boolean;
  showSession: boolean;
}

export function createDialogHandoff(): DialogHandoff {
  const handoff = $state({ showNewItem: true, showSession: false });
  return handoff;
}
