<script lang="ts" generics="T extends string">
  interface Props {
    name: string;
    label: string;
    options: readonly { value: T; label: string }[];
    value: T;
    onValueChange: (value: T) => void;
  }

  let { name, label, options, value, onValueChange }: Props = $props();
</script>

<div class="inline-flex shrink-0 gap-0.5 rounded-md bg-panel-hi p-0.5" role="radiogroup" aria-label={label}>
  {#each options as option (option.value)}
    <label
      class="cursor-pointer rounded-[5px] px-3 py-1 text-[12px] font-medium whitespace-nowrap transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-accent {value === option.value ? 'bg-panel text-t1 shadow-sm ring-1 ring-border' : 'text-t2 hover:text-t1'}"
    >
      <input
        type="radio"
        {name}
        value={option.value}
        checked={value === option.value}
        onchange={() => onValueChange(option.value)}
        class="sr-only"
      />
      {option.label}
    </label>
  {/each}
</div>
