<script lang="ts">
  interface Props {
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
    label: string;
    disabled?: boolean;
    title?: string;
  }

  let { checked, onCheckedChange, label, disabled = false, title }: Props = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  {title}
  class="relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed disabled:opacity-50 {checked ? 'bg-accent' : 'bg-panel-hi ring-1 ring-inset ring-border-s'}"
  onclick={() => onCheckedChange(!checked)}
>
  <span class="inline-block h-3.5 w-3.5 rounded-full shadow-sm transition-transform {checked ? 'translate-x-[16px] bg-on-accent' : 'translate-x-[2px] bg-t3'}"></span>
</button>
