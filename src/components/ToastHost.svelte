<script module lang="ts">
  import { flushSync } from "svelte";
  import { refocusTerminal } from "../lib/focus.svelte";

  /** Where focus was before it entered the stack, restored on Escape and Mod+U. */
  let previousFocus: HTMLElement | null = null;

  function hostElement(): HTMLElement | null {
    return document.querySelector<HTMLElement>("[data-toast-host]");
  }

  function toastElements(): HTMLElement[] {
    return Array.from(hostElement()?.querySelectorAll<HTMLElement>("[data-toast-id]") ?? []);
  }

  function isInsideStack(element: Element | null): boolean {
    return !!element && !!hostElement()?.contains(element);
  }

  function rememberFocus(element: EventTarget | null): void {
    if (element instanceof HTMLElement && element !== document.body && !isInsideStack(element)) {
      previousFocus = element;
    }
  }

  /** Focus the newest visible toast (the Mod+U target). False when none is visible. */
  export function focusNewestToast(): boolean {
    const newest = toastElements().at(-1);
    if (!newest) return false;
    rememberFocus(document.activeElement);
    newest.focus();
    return true;
  }

  /** Mod+U: enter the stack, or leave it when a toast already has focus. */
  export function toggleToastFocus(): boolean {
    if (isInsideStack(document.activeElement)) {
      leaveToasts();
      return true;
    }
    return focusNewestToast();
  }

  function leaveToasts(): void {
    const target = previousFocus;
    previousFocus = null;
    if (target?.isConnected && !isInsideStack(target)) {
      target.focus();
      if (document.activeElement === target) return;
    }
    if (isInsideStack(document.activeElement)) (document.activeElement as HTMLElement).blur();
    refocusTerminal();
  }

  /** After a toast closes from inside the stack, keep the keyboard in the stack while it has toasts. */
  function focusAfterClose(): void {
    flushSync();
    const newest = toastElements().at(-1);
    if (newest) {
      newest.focus();
      return;
    }
    previousFocus = null;
    refocusTerminal();
  }
</script>

<script lang="ts">
  import {
    getToasts,
    holdToast,
    releaseToast,
    requestDismissToast,
    runToastAction,
    type Toast,
    type ToastAction,
    type ToastKind,
  } from "../lib/toasts.svelte";
  import { MOD_LABEL } from "../lib/keyboard";

  let { onboarding = false }: { onboarding?: boolean } = $props();

  const KIND_CLASS: Record<ToastKind, string> = {
    error: "bg-toast-error",
    warning: "bg-toast-warning",
    info: "bg-toast-info",
    success: "bg-toast-success",
  };

  const visible = $derived(getToasts().filter((toast) => !(onboarding && toast.hiddenDuringOnboarding)));
  let focusedId = $state<string | null>(null);
  const hasFocusedToast = $derived(visible.some((toast) => toast.id === focusedId));

  function keyLabel(key: string): string {
    return key === "Enter" ? "↵" : key.toUpperCase();
  }

  function legend(toast: Toast): string {
    const parts = toast.actions.map((action) => `${keyLabel(action.key)} ${action.label.toLowerCase()}`);
    if (toast.dismiss !== "disabled") parts.push(`D ${toast.dismissLabel.toLowerCase()}`);
    parts.push("C copy");
    return parts.join(" · ");
  }

  function keycap(toast: Toast, newest: boolean): string | null {
    if (toast.id === focusedId) return "Esc";
    return newest && !hasFocusedToast ? `${MOD_LABEL}U` : null;
  }

  function footer(toast: Toast): string {
    const parts = toast.id === focusedId ? [legend(toast)] : [];
    if (toast.countdownLabel) parts.push(toast.countdownLabel);
    return parts.join(" · ");
  }

  function close(closed: boolean): void {
    if (closed && isInsideStack(document.activeElement)) focusAfterClose();
  }

  function moveFocus(from: HTMLElement, step: number): void {
    const elements = toastElements();
    elements[elements.indexOf(from) + step]?.focus();
  }

  function onKeydown(event: KeyboardEvent, toast: Toast): void {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
    const element = event.currentTarget as HTMLElement;

    if (key === "Escape") leaveToasts();
    else if (key === "ArrowUp") moveFocus(element, -1);
    else if (key === "ArrowDown") moveFocus(element, 1);
    else if (key === "c") void navigator.clipboard?.writeText(toast.message);
    else if (key === "d") close(requestDismissToast(toast.id));
    else if (toast.actions.some((action) => action.key === key)) close(runToastAction(toast.id, key));
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function onFocusIn(event: FocusEvent, toast: Toast): void {
    rememberFocus(event.relatedTarget);
    if (focusedId !== toast.id) {
      if (focusedId) releaseToast(focusedId, "focus");
      focusedId = toast.id;
      holdToast(toast.id, "focus");
    }
  }

  function onFocusOut(event: FocusEvent, toast: Toast): void {
    if ((event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) return;
    releaseToast(toast.id, "focus");
    if (focusedId === toast.id) focusedId = null;
  }

  function onActionClick(event: MouseEvent, toast: Toast, action: ToastAction): void {
    event.stopPropagation();
    close(runToastAction(toast.id, action.key));
  }

  function onDismissClick(event: MouseEvent, toast: Toast): void {
    event.stopPropagation();
    close(requestDismissToast(toast.id));
  }

  function underlinesInitial(label: string, key: string): boolean {
    return label.charAt(0).toLowerCase() === key;
  }
</script>

{#snippet buttonLabel(label: string, key: string)}
  {#if underlinesInitial(label, key)}<span class="underline">{label.charAt(0)}</span>{label.slice(1)}{:else}{label}{/if}{#if key === "Enter"}<span class="ml-1 text-on-toast/70">↵</span>{/if}
{/snippet}

<div data-toast-host class="pointer-events-none fixed bottom-4 left-4 z-[100] flex w-[28rem] max-w-[calc(100vw-2rem)] flex-col gap-2">
  {#each visible as toast, index (toast.id)}
    {@const cap = keycap(toast, index === visible.length - 1)}
    {@const foot = footer(toast)}
    <!-- Keys are handled on the focused toast; clicking the body is the mouse dismiss and must not steal focus. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      data-toast-id={toast.id}
      role="alertdialog"
      aria-label={toast.message}
      tabindex="-1"
      class="pointer-events-auto rounded-lg px-4 py-3 shadow-lg outline-none focus:ring-2 focus:ring-on-toast/70 focus:ring-inset {KIND_CLASS[toast.kind]} {toast.dismiss === 'disabled' ? '' : 'cursor-pointer'}"
      onmousedown={(event) => event.preventDefault()}
      onclick={() => close(requestDismissToast(toast.id))}
      onkeydown={(event) => onKeydown(event, toast)}
      onfocusin={(event) => onFocusIn(event, toast)}
      onfocusout={(event) => onFocusOut(event, toast)}
      onmouseenter={() => holdToast(toast.id, "hover")}
      onmouseleave={() => releaseToast(toast.id, "hover")}
    >
      <div class="flex items-start gap-3">
        <p class="min-w-0 flex-1 text-sm text-on-toast font-mono wrap-anywhere">
          {toast.message}{#if toast.detail}<span class="text-on-toast/70">{` · ${toast.detail}`}</span>{/if}{#if toast.count > 1}<span class="ml-2 text-on-toast/70">x {toast.count}</span>{/if}
        </p>
        {#if cap}
          <kbd class="mt-0.5 shrink-0 rounded border border-b-2 border-on-toast/20 bg-on-toast/15 px-1.5 py-0.5 font-sans text-[11px] leading-none font-medium text-on-toast/90">{cap}</kbd>
        {/if}
      </div>
      {#if toast.actions.length > 0}
        <div class="mt-2 flex gap-2">
          {#each toast.actions as action (action.key)}
            <button
              type="button"
              tabindex="-1"
              class="rounded bg-on-toast/20 px-3 py-1 text-xs font-medium text-on-toast hover:bg-on-toast/30"
              onclick={(event) => onActionClick(event, toast, action)}
            >{@render buttonLabel(action.label, action.key)}</button>
          {/each}
          {#if toast.dismiss !== "disabled"}
            <button
              type="button"
              tabindex="-1"
              class="rounded bg-on-toast/10 px-3 py-1 text-xs font-medium text-on-toast/80 hover:bg-on-toast/20"
              onclick={(event) => onDismissClick(event, toast)}
            >{@render buttonLabel(toast.dismissLabel, "d")}</button>
          {/if}
        </div>
      {/if}
      {#if foot}
        <p class="mt-1 text-xs text-on-toast/70">{foot}</p>
      {/if}
    </div>
  {/each}
</div>
