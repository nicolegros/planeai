<script lang="ts">
  interface Props {
    values?: string[];
    placeholder?: string;
    class?: string;
  }

  let { values = $bindable([]), placeholder = "", class: className = "" }: Props = $props();

  let inputValue = $state("");
  let inputEl = $state<HTMLInputElement | null>(null);

  function addPill() {
    const trimmed = inputValue.trim();
    if (trimmed && !values.includes(trimmed)) {
      values = [...values, trimmed];
    }
    inputValue = "";
  }

  function removePill(index: number) {
    values = values.filter((_, i) => i !== index);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      addPill();
    } else if (e.key === "Backspace" && inputValue === "" && values.length > 0) {
      values = values.slice(0, -1);
    }
  }

  function focusInput() {
    inputEl?.focus();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex flex-wrap items-center gap-1.5 w-full rounded border border-border bg-panel px-2 py-1.5 text-sm text-t1 focus-within:ring-1 focus-within:ring-accent {className}"
  onclick={focusInput}
>
  {#each values as pill, i (pill + i)}
    <span class="inline-flex items-center gap-1 rounded bg-panel-hi px-2 py-0.5 text-xs text-t2">
      {pill}
      <button
        type="button"
        class="text-t3 hover:text-t1 transition-colors leading-none"
        onclick={() => removePill(i)}
        aria-label="Remove {pill}"
      >×</button>
    </span>
  {/each}
  <input
    bind:this={inputEl}
    bind:value={inputValue}
    onkeydown={handleKeydown}
    {placeholder}
    autocomplete="off"
    autocorrect="off"
    autocapitalize="off"
    spellcheck={false}
    data-form-type="other"
    class="flex-1 min-w-[80px] bg-transparent outline-none placeholder:text-t3 py-0.5 text-sm"
  />
</div>
