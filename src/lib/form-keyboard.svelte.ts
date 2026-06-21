/**
 * Form keyboard controller — vim-like normal/insert mode for forms.
 *
 * Normal mode: mnemonic keys focus fields; toggle keys flip switches.
 * Insert mode: entered when a text field is focused; typing edits text.
 * Esc in insert → normal (blurs active field, returns focus to wrapper).
 * Esc in normal → calls onDismiss (close form).
 * Tab/Shift-Tab cycles fields naturally; focusin auto-detects insert.
 */

export type FormMode = "normal" | "insert";

export interface FieldBinding {
  /** The mnemonic key (lowercase) that activates this field */
  key: string;
  /** Reference to the focusable element */
  ref?: () => HTMLElement | null;
  /** If true, pressing the key toggles a value instead of focusing */
  toggle?: () => void;
}

export function createFormKeyboardController(
  bindings: () => FieldBinding[],
  opts: { wrapper: () => HTMLElement | null; onDismiss?: () => void },
) {
  let mode: FormMode = $state("normal");

  function isTextField(el: Element | null): boolean {
    if (!el) return false;
    if (
      el.tagName === "INPUT" &&
      !["checkbox", "radio", "button"].includes((el as HTMLInputElement).type)
    )
      return true;
    if (el.tagName === "TEXTAREA") return true;
    if (el.closest("[role='combobox']")) return true;
    return false;
  }

  function enterInsert() {
    mode = "insert";
  }

  function enterNormal() {
    mode = "normal";
    (document.activeElement as HTMLElement)?.blur?.();
    opts.wrapper()?.focus();
  }

  function handleKeydown(e: KeyboardEvent) {
    // ⌘Enter — let it bubble for submit
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) return;

    // Esc
    if (e.key === "Escape") {
      // If inside an open combobox, let it close its dropdown first
      const el = document.activeElement as HTMLElement | null;
      if (el?.getAttribute("aria-expanded") === "true") return;
      e.preventDefault();
      e.stopPropagation();
      if (mode === "insert") {
        enterNormal();
      } else {
        opts.onDismiss?.();
      }
      return;
    }

    // In insert mode, don't intercept typing
    if (mode === "insert") return;

    // Normal mode: check mnemonic bindings
    const key = e.key.toLowerCase();
    const binding = bindings().find((b) => b.key === key);
    if (binding) {
      e.preventDefault();
      if (binding.toggle) {
        binding.toggle();
      } else if (binding.ref) {
        const el = binding.ref();
        if (el) el.focus();
      }
    }
  }

  function handleFocusin(e: FocusEvent) {
    if (isTextField(e.target as Element)) enterInsert();
  }

  return {
    get mode() {
      return mode;
    },
    handleKeydown,
    handleFocusin,
  };
}
