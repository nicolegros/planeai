<script lang="ts" module>
  export type MenuItem =
    | {
        label: string;
        danger?: boolean;
        /** Renders a check mark; `radio` marks the item as one of mutually exclusive choices. */
        checked?: boolean;
        radio?: boolean;
        disabled?: boolean;
        title?: string;
        onSelect: () => void;
      }
    | { label: string; children: MenuItem[] }
    | { separator: true };
</script>

<script lang="ts">
  import { flushSync } from "svelte";
  import { Check } from "@lucide/svelte";

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onClose: () => void;
  }

  let { x, y, items, onClose }: Props = $props();

  let openSubmenuIndex = $state<number | null>(null);
  let submenuPosition = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  let menuEl = $state<HTMLElement | undefined>(undefined);
  let submenuEl = $state<HTMLElement | undefined>(undefined);

  function isParent(item: MenuItem): item is { label: string; children: MenuItem[] } {
    return "children" in item;
  }

  function isSeparator(item: MenuItem): item is { separator: true } {
    return "separator" in item;
  }

  type LeafItem = Extract<MenuItem, { onSelect: () => void }>;

  // Reserve a check column only for menus that have checkable items.
  function hasCheckColumn(list: MenuItem[]): boolean {
    return list.some((item) => "checked" in item);
  }

  const submenuItems = $derived<MenuItem[]>(
    openSubmenuIndex !== null && isParent(items[openSubmenuIndex])
      ? (items[openSubmenuIndex] as { label: string; children: MenuItem[] }).children
      : []
  );

  function focusableItems(container: HTMLElement | undefined): HTMLButtonElement[] {
    return [...(container?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
  }

  $effect(() => {
    if (menuEl) focusableItems(menuEl)[0]?.focus();
  });

  function openSubmenu(index: number, trigger: HTMLElement) {
    const item = items[index];
    if (!isParent(item) || item.children.length === 0) {
      openSubmenuIndex = null;
      return;
    }
    openSubmenuIndex = index;
    const rect = trigger.getBoundingClientRect();
    // Overlap parent by 4px to prevent dead zone between menus
    submenuPosition = { x: rect.right - 4, y: rect.top };
  }

  function openSubmenuAndFocus(index: number, trigger: HTMLElement) {
    openSubmenu(index, trigger);
    flushSync();
    focusableItems(submenuEl)[0]?.focus();
  }

  function closeSubmenu() {
    const trigger = menuEl?.querySelector<HTMLElement>(`[data-submenu-index="${openSubmenuIndex}"]`);
    openSubmenuIndex = null;
    trigger?.focus();
  }

  // Keys stay inside the menu so global shortcuts (e.g. sidebar navigation) do not fire behind it.
  function handleKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    const inSubmenu = !!submenuEl?.contains(document.activeElement);
    if (e.key === "Escape" || (e.key === "ArrowLeft" && inSubmenu)) {
      e.preventDefault();
      if (inSubmenu) closeSubmenu();
      else onClose();
      return;
    }
    const trigger = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>("[data-submenu-index]");
    if (trigger && (e.key === "ArrowRight" || e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      openSubmenuAndFocus(Number(trigger.dataset.submenuIndex), trigger);
      return;
    }
    // Tab cycles like the arrows so focus never leaves the open menu.
    const forward = e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey);
    const backward = e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey);
    if (!forward && !backward) return;
    e.preventDefault();
    const buttons = focusableItems(inSubmenu ? submenuEl : menuEl);
    if (buttons.length === 0) return;
    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const step = forward ? 1 : -1;
    const next = current === -1 ? (forward ? 0 : buttons.length - 1) : (current + step + buttons.length) % buttons.length;
    buttons[next].focus();
  }
</script>

{#snippet leafItem(item: LeafItem, checkColumn: boolean, onHover?: () => void)}
  <!-- Hover moves focus so the pointer and the keyboard share one highlighted row; disabled rows ignore hover. -->
  <button
    class="w-full text-left px-3 py-1.5 flex items-center gap-2 outline-none {item.disabled ? 'text-t3 cursor-not-allowed' : 'hover:bg-panel-hi focus-visible:bg-panel-hi'} {!item.disabled && item.danger ? 'text-red-600 dark:text-red-400' : ''} {!item.disabled && !item.danger ? 'text-t2' : ''}"
    role={"checked" in item ? (item.radio ? "menuitemradio" : "menuitemcheckbox") : "menuitem"}
    aria-checked={"checked" in item ? !!item.checked : undefined}
    disabled={item.disabled}
    title={item.title}
    onclick={() => { item.onSelect(); onClose(); }}
    onmouseenter={(e) => { if (item.disabled) return; e.currentTarget.focus(); onHover?.(); }}
  >
    {#if checkColumn}
      <span class="size-3.5 shrink-0 flex items-center justify-center">{#if item.checked}<Check class="size-3.5" />{/if}</span>
    {/if}
    {item.label}
  </button>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-50" onclick={onClose} oncontextmenu={(e) => { e.preventDefault(); onClose(); }} onkeydown={handleKeydown}>
  <div
    bind:this={menuEl}
    class="absolute rounded border border-border bg-panel shadow-lg py-1 text-sm min-w-40 w-max"
    style="left: {x}px; top: {y}px;"
    role="menu"
    tabindex="-1"
  >
    {#each items as item, index}
      {#if isSeparator(item)}
        <div class="my-1 h-px bg-border" role="separator"></div>
      {:else if isParent(item)}
        <button
          class="w-full text-left px-3 py-1.5 hover:bg-panel-hi focus-visible:bg-panel-hi outline-none text-t2 flex items-center justify-between cursor-default"
          role="menuitem"
          aria-haspopup="menu"
          aria-expanded={openSubmenuIndex === index}
          data-submenu-index={index}
          onclick={(e) => { e.stopPropagation(); openSubmenuAndFocus(index, e.currentTarget); }}
          onmouseenter={(e) => { e.currentTarget.focus(); openSubmenu(index, e.currentTarget); }}
        >
          <span>{item.label}</span>
          <span class="text-t3 text-xs">›</span>
        </button>
      {:else}
        {@render leafItem(item, hasCheckColumn(items), () => { openSubmenuIndex = null; })}
      {/if}
    {/each}
  </div>

  <!-- Submenu (overlaps parent by 4px to prevent gap) -->
  {#if submenuItems.length > 0}
    <div
      bind:this={submenuEl}
      class="absolute rounded border border-border bg-panel shadow-lg py-1 text-sm min-w-36 w-max"
      style="left: {submenuPosition.x}px; top: {submenuPosition.y}px;"
      role="menu"
      tabindex="-1"
    >
      {#each submenuItems as child}
        {#if isSeparator(child)}
          <div class="my-1 h-px bg-border" role="separator"></div>
        {:else if !isParent(child)}
          {@render leafItem(child, hasCheckColumn(submenuItems))}
        {/if}
      {/each}
    </div>
  {/if}
</div>
