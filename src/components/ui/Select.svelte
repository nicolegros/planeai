<script lang="ts">
  import { Combobox } from "bits-ui";

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
    class?: string;
  }

  let { items, value = $bindable(""), onValueChange, onInput, onkeydown, placeholder = "", emptyText = "No results", class: className = "" }: Props = $props();

  let search = $state("");
  let open = $state(false);

  const inputValue = $derived(
    open ? undefined : (items.find((i) => i.value === value)?.label ?? (value || undefined))
  );

  const filtered = $derived(
    search === "" ? items : items.filter((i) => i.label.toLowerCase().includes(search.toLowerCase()))
  );
</script>

<Combobox.Root type="single" allowDeselect={false} bind:value bind:open {onValueChange} {inputValue} onOpenChangeComplete={(o) => { if (!o) search = ""; }}>
  <Combobox.Input
    onfocus={() => { open = true; }}
    oninput={(e) => { search = e.currentTarget.value; onInput?.(search); }}
    onkeydown={(e) => {
      if (e.key === "Enter" && filtered.length === 1) { e.preventDefault(); value = filtered[0].value; onValueChange?.(value); open = false; }
      onkeydown?.(e);
    }}
    {placeholder}
    autocomplete="off"
    autocorrect="off"
    autocapitalize="off"
    spellcheck={false}
    data-form-type="other"
    class="w-full rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm text-surface-900 placeholder:text-surface-400 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50 dark:placeholder:text-surface-500 {className}"
  />
  <Combobox.Portal>
    <Combobox.Content loop class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-surface-200 bg-surface-50 shadow-lg dark:border-surface-700 dark:bg-surface-900" sideOffset={4}>
      {#each filtered as item (item.value)}
        <Combobox.Item value={item.value} label={item.label} class="flex items-center justify-between cursor-pointer px-3 py-2 text-sm text-surface-700 data-[highlighted]:bg-surface-100 dark:text-surface-300 dark:data-[highlighted]:bg-surface-800">
          <span>{item.label}</span>
          {#if item.remote}<span class="rounded bg-surface-200 px-1.5 py-0.5 text-[10px] text-surface-500 dark:bg-surface-700 dark:text-surface-400">remote</span>{/if}
        </Combobox.Item>
      {:else}
        <span class="block px-3 py-2 text-sm text-surface-600 dark:text-surface-400">{emptyText}</span>
      {/each}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
