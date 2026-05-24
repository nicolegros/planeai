<script lang="ts">
  import { Combobox } from "bits-ui";

  interface Item {
    value: string;
    label: string;
  }

  interface Props {
    items: Item[];
    value?: string;
    onValueChange?: (value: string) => void;
    onInput?: (search: string) => void;
    placeholder?: string;
    class?: string;
  }

  let { items, value = $bindable(""), onValueChange, onInput, placeholder = "", class: className = "" }: Props = $props();

  let search = $state("");

  const filtered = $derived(
    search === "" ? items : items.filter((i) => i.label.toLowerCase().includes(search.toLowerCase()))
  );
</script>

<Combobox.Root type="single" bind:value {onValueChange} onOpenChangeComplete={(o) => { if (!o) search = ""; }}>
  <Combobox.Input
    oninput={(e) => { search = e.currentTarget.value; onInput?.(search); }}
    {placeholder}
    autocomplete="off"
    autocorrect="off"
    autocapitalize="off"
    spellcheck={false}
    data-form-type="other"
    class="w-full rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm text-surface-900 placeholder:text-surface-400 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50 dark:placeholder:text-surface-500 {className}"
  />
  <Combobox.Portal>
    <Combobox.Content class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-surface-200 bg-surface-50 shadow-lg dark:border-surface-700 dark:bg-surface-900" sideOffset={4}>
      {#each filtered as item (item.value)}
        <Combobox.Item value={item.value} label={item.label} class="cursor-pointer px-3 py-2 text-sm text-surface-700 data-[highlighted]:bg-surface-100 dark:text-surface-300 dark:data-[highlighted]:bg-surface-800">
          {item.label}
        </Combobox.Item>
      {:else}
        <span class="block px-3 py-2 text-sm text-surface-400">No results</span>
      {/each}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
