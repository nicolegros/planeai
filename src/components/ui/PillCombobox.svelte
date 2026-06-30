<script lang="ts">
  import { Combobox } from "bits-ui";

  interface Item {
    value: string;
    label: string;
  }

  interface Props {
    items: Item[];
    values?: string[];
    placeholder?: string;
    emptyText?: string;
    class?: string;
  }

  let { items, values = $bindable([]), placeholder = "", emptyText = "No results", class: className = "" }: Props = $props();

  let search = $state("");
  let open = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);
  let contentRef = $state<HTMLElement | null>(null);
  // Internal dummy value — we never actually use bits-ui's value tracking
  let internalValue = $state("");

  const filtered = $derived(
    (search === "" ? items : items.filter((i) => i.label.toLowerCase().includes(search.toLowerCase())))
      .filter((i) => !values.includes(i.value))
  );

  function removePill(val: string) {
    values = values.filter((v) => v !== val);
  }

  function labelFor(val: string): string {
    return items.find((i) => i.value === val)?.label ?? val;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (open) {
        e.stopPropagation();
        open = false;
        search = "";
      }
      // Let Escape propagate to the form-keyboard controller when dropdown is already closed
      return;
    }
    if (e.key === "Backspace" && search === "" && values.length > 0) {
      values = values.slice(0, -1);
    }
  }

  function handleSelect(val: string | undefined) {
    if (val && !values.includes(val)) {
      values = [...values, val];
    }
    // Reset internal state so the combobox doesn't think it has a value selected
    internalValue = "";
    search = "";
    // Keep dropdown open and refocus input for rapid multi-select
    requestAnimationFrame(() => {
      inputEl?.focus();
      open = true;
    });
  }

  function focusInput() {
    inputEl?.focus();
    open = true;
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
</script>

<Combobox.Root
  type="single"
  allowDeselect
  bind:value={internalValue}
  bind:open
  onValueChange={handleSelect}
  inputValue={open ? undefined : ""}
  onOpenChangeComplete={(o) => { if (!o) { search = ""; internalValue = ""; } }}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="flex flex-wrap items-center gap-1.5 w-full rounded border border-border bg-panel px-2 py-1.5 text-sm text-t1 focus-within:ring-1 focus-within:ring-accent {className}"
    onclick={focusInput}
  >
    {#each values as val (val)}
      <span class="inline-flex items-center gap-1 rounded bg-panel-hi px-2 py-0.5 text-xs text-t2">
        {labelFor(val)}
        <button
          type="button"
          class="text-t3 hover:text-t1 transition-colors leading-none"
          onclick={(e) => { e.stopPropagation(); removePill(val); }}
          aria-label="Remove {labelFor(val)}"
        >×</button>
      </span>
    {/each}
    <Combobox.Input
      bind:ref={inputEl}
      onfocus={() => { open = true; }}
      oninput={(e) => { search = e.currentTarget.value; open = true; }}
      onkeydown={handleKeydown}
      placeholder={values.length === 0 ? placeholder : ""}
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck={false}
      data-form-type="other"
      class="flex-1 min-w-[80px] bg-transparent outline-none placeholder:text-t3 py-0.5 text-sm"
    />
  </div>
  <Combobox.Portal>
    <Combobox.Content loop bind:ref={contentRef} class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-border bg-panel shadow-lg" sideOffset={4}>
      {#each filtered as item (item.value)}
        <Combobox.Item value={item.value} label={item.label} class="cursor-pointer px-3 py-2 text-sm text-t2 data-[highlighted]:bg-panel-hi">
          {item.label}
        </Combobox.Item>
      {:else}
        <span class="block px-3 py-2 text-sm text-t3">{emptyText}</span>
      {/each}
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
