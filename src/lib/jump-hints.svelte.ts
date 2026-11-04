import { IS_MAC, MOD_LABEL } from "./keyboard";
import { getVisibleNavigationOrder } from "./sidebar-model-store.svelte";

export const JUMP_HINT_SHOW_DELAY_MS = 200;
const JUMP_HINT_COUNT = 9;

type ModifierKey = "Meta" | "Control";

/** Calls onChange(true) once modKey has been held alone for the delay, and onChange(false) when the hold ends. */
export function createModifierHold(modKey: ModifierKey, onChange: (shown: boolean) => void) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let shown = false;

  function reset(): void {
    clearTimeout(timer);
    timer = undefined;
    if (shown) {
      shown = false;
      onChange(false);
    }
  }

  function keydown(e: KeyboardEvent): void {
    if (e.repeat && e.key === modKey) return;
    reset();
    const otherMod = modKey === "Meta" ? e.ctrlKey : e.metaKey;
    if (e.key !== modKey || e.shiftKey || e.altKey || otherMod) return;
    timer = setTimeout(() => {
      timer = undefined;
      shown = true;
      onChange(true);
    }, JUMP_HINT_SHOW_DELAY_MS);
  }

  return { keydown, keyup: reset, reset };
}

/** Shortcut label per navigation target, for the targets Mod+1-9 reach. */
export function jumpHintLabels(order: readonly string[], modLabel: string): Map<string, string> {
  return new Map(order.slice(0, JUMP_HINT_COUNT).map((id, i) => [id, `${modLabel}${i + 1}`]));
}

let visible = $state(false);
const labels = $derived(jumpHintLabels(getVisibleNavigationOrder(), MOD_LABEL));

/** The Mod+digit label of a sidebar navigation target while the modifier is held, else undefined. */
export function jumpHintFor(targetId: string): string | undefined {
  return visible ? labels.get(targetId) : undefined;
}

/** Watches the platform modifier without consuming any key event. Returns the cleanup. */
export function installJumpHints(): () => void {
  const hold = createModifierHold(IS_MAC ? "Meta" : "Control", (shown) => (visible = shown));
  const onVisibility = () => {
    if (document.visibilityState === "hidden") hold.reset();
  };
  // Capture so focused content that stops propagation, such as xterm, cannot hide a keydown.
  window.addEventListener("keydown", hold.keydown, true);
  window.addEventListener("keyup", hold.keyup, true);
  window.addEventListener("blur", hold.reset);
  document.addEventListener("visibilitychange", onVisibility);
  return () => {
    hold.reset();
    window.removeEventListener("keydown", hold.keydown, true);
    window.removeEventListener("keyup", hold.keyup, true);
    window.removeEventListener("blur", hold.reset);
    document.removeEventListener("visibilitychange", onVisibility);
  };
}
