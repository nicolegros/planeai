<script lang="ts">
  import { Combobox } from "bits-ui";
  import { ensureFirstItemHighlighted, selectFirstIfNoHighlight } from "./combobox-highlight";

  interface Item {
    value: string;
    label: string;
    remote?: boolean;
  }

  interface Props {
    items: Item[];
    value?: string;
    onValueChange?: (value: string) => void;
    onInput?: (search: string) => void;
    onkeydown?: (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => void;
    placeholder?: string;
    emptyText?: string;
    allowDeselect?: boolean;
    class?: string;
  }

  let { items, value = $bindable(""), onValueChange, onInput, onkeydown, placeholder = "", emptyText = "No results", allowDeselect = false, class: className = "" }: Props = $props();

  let search = $state("");
  let open = $state(false);
  let contentRef = $state<HTMLElement | null>(null);

  const inputValue = $derived(
    open ? undefined : (items.find((i) => i.value === value)?.label ?? (value || undefined))
  );

  const filtered = $derived(
    search === "" ? items : items.filter((i) => i.label.toLowerCase().includes(search.toLowerCase()))
  );

  function handleKeydown(e: KeyboardEvent & { currentTarget: HTMLInputElement }) {
    if (e.key === "Escape") {
      if (open) {
        e.stopPropagation();
        open = false;
        search = "";
      }
      // When closed, let Escape propagate to form-keyboard controller
      return;
    }
    if (e.key === "Backspace" && allowDeselect && e.currentTarget.value === "" && value) {
      clearValue();
      return;
    }
    if (e.key === "Enter" && open && filtered.length > 0) {
      const applied = selectFirstIfNoHighlight(e, contentRef, filtered[0].value, (v) => {
        value = v;
        onValueChange?.(v);
        open = false;
        search = "";
      });
      if (applied) return;
    }
    onkeydown?.(e);
  }

  function clearValue() {
    value = "";
    onValueChange?.("");
  }

  $effect(() => {
    const node = contentRef;
    if (!node) return;
    let hasInteracted = false;
    const onKey = () => { hasInteracted = true; };
    node.getRootNode().addEventListener("keydown", onKey, true);
    const ob = new MutationObserver(() => {
      if (!hasInteracted) return;
      const el = node.querySelector("[data-highlighted]");
      if (el) el.scrollIntoView({ block: "nearest" });
    });
    ob.observe(node, { attributes: true, subtree: true, attributeFilter: ["data-highlighted"] });
    return () => { ob.disconnect(); node.getRootNode().removeEventListener("keydown", onKey, true); };
  });

  // When filtered results change or dropdown opens, ensure the first item is highlighted
  // so the user always sees which item will be selected on Enter.
  $effect(() => {
    if (!open || filtered.length === 0) return;
    ensureFirstItemHighlighted(contentRef);
  });
</script>

<Combobox.Root type="single" {allowDeselect} bind:value bind:open {onValueChange} {inputValue} onOpenChangeComplete={(o) => { if (!o) search = ""; }}>
  <div class="relative">
    <Combobox.Input
      onfocus={() => { open = true; }}
      oninput={(e) => { search = e.currentTarget.value; onInput?.(search); }}
      onkeydown={handleKeydown}
      {placeholder}
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck={false}
      data-form-type="other"
      class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent {allowDeselect && value ? 'pr-8' : ''} {className}"
    />
    {#if allowDeselect && value}
      <button
        type="button"
        class="absolute right-2 top-1/2 -translate-y-1/2 text-t3 hover:text-t1 transition-colors text-sm leading-none"
        onclick={clearValue}
        aria-label="Clear selection"
      >×</button>
    {/if}
  </div>
  <Combobox.Portal>
    <Combobox.Content loop bind:ref={contentRef} class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-border bg-panel shadow-lg" sideOffset={4}>
      {#each filtered as item (item.value)}
        <Combobox.Item value={item.value} label={item.label} class="flex items-center justify-between cursor-pointer px-3 py-2 text-sm text-t2 data-[highlighted]:bg-panel-hi">
          <span>{item.label}</span>
          {#if item.remote}<span class="rounded bg-panel-hi px-1.5 py-0.5 text-[10px] text-t3">remote</span>{/if}
        </Combobox.Item>
      {:else}
        <span class="block px-3 py-2 text-sm text-t3">{emptyText}</span>
      {/each}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
