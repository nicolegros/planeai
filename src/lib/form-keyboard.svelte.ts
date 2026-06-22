/**
 * Form keyboard controller — vim-like normal/insert mode for forms.
 *
 * Normal mode: mnemonic keys focus fields; toggle keys flip switches.
 * Insert mode: entered when a text field is focused; typing edits text.
 * Esc returns to normal mode (blurs active field).
 * Tab/Shift-Tab cycles fields in either mode.
 */

export type FormMode = "normal" | "insert";

export interface FieldBinding {
  /** The mnemonic key (lowercase) that activates this field */
  key: string;
  /** Reference to the focusable element */
  ref: () => HTMLElement | null;
  /** If true, pressing the key toggles a value instead of focusing */
  toggle?: () => void;
}

export interface FormKeyboardController {
  mode: FormMode;
  handleKeydown: (e: KeyboardEvent) => void;
  destroy: () => void;
}

export function createFormKeyboardController(
  bindings: () => FieldBinding[],
  onModeChange?: (mode: FormMode) => void,
): FormKeyboardController {
  let mode: FormMode = $state("normal");

  function isTextField(el: Element | null): boolean {
    if (!el) return false;
    if (el.tagName === "INPUT" && !["checkbox", "radio", "button"].includes((el as HTMLInputElement).type)) return true;
    if (el.tagName === "TEXTAREA") return true;
    if (el.closest("[role='combobox']")) return true;
    return false;
  }

  function enterInsert() {
    if (mode === "insert") return;
    mode = "insert";
    onModeChange?.("insert");
  }

  function enterNormal() {
    if (mode === "normal") return;
    mode = "normal";
    onModeChange?.("normal");
    (document.activeElement as HTMLElement)?.blur?.();
  }

  // Listen for focus events to auto-detect insert mode
  function onFocusIn(e: FocusEvent) {
    if (isTextField(e.target as Element)) enterInsert();
  }

  function onFocusOut(e: FocusEvent) {
    // If nothing is focused after this event, stay in current mode
    // (the handleKeydown Esc will handle explicit exit)
  }

  document.addEventListener("focusin", onFocusIn);

  function handleKeydown(e: KeyboardEvent) {
    // Esc always returns to normal
    if (e.key === "Escape" && mode === "insert") {
      e.preventDefault();
      enterNormal();
      return;
    }

    // In insert mode, don't intercept typing
    if (mode === "insert") return;

    // Normal mode: check mnemonic bindings
    const fields = bindings();
    const key = e.key.toLowerCase();
    const binding = fields.find(b => b.key === key);

    if (binding) {
      e.preventDefault();
      if (binding.toggle) {
        binding.toggle();
      } else {
        const el = binding.ref();
        if (el) {
          el.focus();
          // Focus will trigger focusin → enterInsert if it's a text field
        }
      }
    }
  }

  function destroy() {
    document.removeEventListener("focusin", onFocusIn);
  }

  return {
    get mode() { return mode; },
    handleKeydown,
    destroy,
  };
}
