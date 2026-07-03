/**
 * Prompt queue primitive — manages a FIFO queue of prompts shown one-at-a-time,
 * with deduplication and focus coordination for the displaying component.
 *
 * Usage:
 *   const queue = createPromptQueue<MyPrompt>((a, b) => a.key === b.key);
 *   queue.push(item);           // enqueue (deduplicates)
 *   queue.getCurrent();         // reactive getter for the active prompt
 *   queue.dismiss();            // drop current, advance to next
 *   queue.registerFocus(fn);    // component registers its focus handler
 */

export interface PromptQueue<T> {
  getCurrent(): T | null;
  push(item: T): void;
  dismiss(): void;
  registerFocus(fn: () => void): void;
  unregisterFocus(): void;
  focus(): void;
}

/**
 * Create a prompt queue. Items are deduplicated using the provided `eq` function.
 */
export function createPromptQueue<T>(eq: (a: T, b: T) => boolean): PromptQueue<T> {
  let queue = $state<T[]>([]);
  let current = $state<T | null>(null);
  let focusFn: (() => void) | null = null;

  function advance() {
    if (queue.length > 0) {
      current = queue.shift()!;
    } else {
      current = null;
    }
  }

  return {
    getCurrent(): T | null {
      return current;
    },

    push(item: T): void {
      if (current && eq(current, item)) return;
      if (queue.some((p) => eq(p, item))) return;
      if (!current) {
        current = item;
      } else {
        queue.push(item);
      }
    },

    dismiss(): void {
      current = null;
      advance();
    },

    registerFocus(fn: () => void): void {
      focusFn = fn;
    },

    unregisterFocus(): void {
      focusFn = null;
    },

    focus(): void {
      focusFn?.();
    },
  };
}
