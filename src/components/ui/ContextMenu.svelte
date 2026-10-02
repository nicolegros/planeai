<script lang="ts" module>
  export type MenuItem =
    | { label: string; danger?: boolean; checked?: boolean; disabled?: boolean; title?: string; onSelect: () => void }
    | { label: string; children: MenuItem[] }
    | { separator: true };
</script>

<script lang="ts">
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

  function isParent(item: MenuItem): item is { label: string; children: MenuItem[] } {
    return "children" in item;
  }

  function isSeparator(item: MenuItem): item is { separator: true } {
    return "separator" in item;
  }

  // Reserve a check column only for menus that have checkable items.
  const hasChecks = $derived(items.some((item) => "checked" in item));

  const submenuItems = $derived<MenuItem[]>(
    openSubmenuIndex !== null && isParent(items[openSubmenuIndex])
      ? (items[openSubmenuIndex] as { label: string; children: MenuItem[] }).children
      : []
  );

  function handleItemHover(index: number, event: MouseEvent) {
    const item = items[index];
    if (isParent(item) && item.children.length > 0) {
      openSubmenuIndex = index;
      const target = event.currentTarget as HTMLElement;
      const rect = target.getBoundingClientRect();
      // Overlap parent by 4px to prevent dead zone between menus
      submenuPosition = { x: rect.right - 4, y: rect.top };
    } else {
      openSubmenuIndex = null;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-50" onclick={onClose} oncontextmenu={(e) => { e.preventDefault(); onClose(); }}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="absolute rounded border border-border bg-panel shadow-lg py-1 text-sm min-w-40 w-max"
    style="left: {x}px; top: {y}px;"
    role="menu"
  >
    {#each items as item, index}
      {#if isSeparator(item)}
        <div class="my-1 h-px bg-border" role="separator"></div>
      {:else if isParent(item)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="w-full text-left px-3 py-1.5 hover:bg-panel-hi text-t2 flex items-center justify-between cursor-default"
          onmouseenter={(e) => handleItemHover(index, e)}
        >
          <span>{item.label}</span>
          <span class="text-t3 text-xs">›</span>
        </div>
      {:else}
        <button
          class="w-full text-left px-3 py-1.5 flex items-center gap-2 {item.disabled ? 'text-t3 cursor-not-allowed' : 'hover:bg-panel-hi'} {!item.disabled && item.danger ? 'text-red-600 dark:text-red-400' : ''} {!item.disabled && !item.danger ? 'text-t2' : ''}"
          role={"checked" in item ? "menuitemcheckbox" : "menuitem"}
          aria-checked={"checked" in item ? !!item.checked : undefined}
          disabled={item.disabled}
          title={item.title}
          onclick={(e) => { if (item.disabled) { e.stopPropagation(); return; } item.onSelect(); onClose(); }}
          onmouseenter={() => { openSubmenuIndex = null; }}
        >
          {#if hasChecks}
            <span class="size-3.5 shrink-0 flex items-center justify-center">{#if item.checked}<Check class="size-3.5" />{/if}</span>
          {/if}
          {item.label}
        </button>
      {/if}
    {/each}
  </div>

  <!-- Submenu (overlaps parent by 4px to prevent gap) -->
  {#if submenuItems.length > 0}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="absolute rounded border border-border bg-panel shadow-lg py-1 text-sm w-36"
      style="left: {submenuPosition.x}px; top: {submenuPosition.y}px;"
    >
      {#each submenuItems as child}
        {#if isSeparator(child)}
          <div class="my-1 h-px bg-border" role="separator"></div>
        {:else if !isParent(child)}
          <button
            class="w-full text-left px-3 py-1.5 hover:bg-panel-hi {child.danger ? 'text-red-600 dark:text-red-400' : 'text-t2'}"
            onclick={() => { child.onSelect(); onClose(); }}
          >
            {child.label}
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>
