<script lang="ts" module>
  export interface Choice<Id extends string = string> {
    id: Id;
    label: string;
    glyph: string;
    key: string;
    disabled?: boolean;
  }
</script>

<script lang="ts" generics="Id extends string">
  import SharedDialog from "./ui/Dialog.svelte";

  interface Props {
    title: string;
    options: readonly Choice<Id>[];
    onChoose: (id: Id) => void;
    onClose: () => void;
  }

  let { title, options, onChoose, onClose }: Props = $props();

  const enabled = $derived(options.filter((o) => !o.disabled));
  let pickedId = $state<Id | null>(null);
  const highlightedId = $derived(
    enabled.some((o) => o.id === pickedId) ? pickedId : (enabled[0]?.id ?? null),
  );

  function move(step: 1 | -1): void {
    if (enabled.length === 0) return;
    const index = enabled.findIndex((o) => o.id === highlightedId);
    pickedId = enabled[(index + step + enabled.length) % enabled.length].id;
  }

  // Capture on window: the dialog leaves focus in the terminal, which would otherwise eat these keys.
  function onKeydown(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter") {
      if (highlightedId) onChoose(highlightedId);
    } else {
      const option = enabled.find((o) => o.key === e.key);
      if (option) onChoose(option.id);
    }
  }
</script>

<svelte:window onkeydowncapture={onKeydown} />

<SharedDialog
  open={true}
  onOpenChange={(open) => {
    if (!open) onClose();
  }}
  {title}
  preventOpenAutoFocus={true}
  class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden"
>
  <div>
    <div class="flex items-center px-[15px] pt-[13px] pb-[11px]">
      <span class="text-[13px] font-semibold text-t1">{title}</span>
      <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
    </div>
    <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
      {#each options as option (option.id)}
        {@const highlighted = option.id === highlightedId}
        <button
          class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] transition-colors disabled:opacity-40 {highlighted ? 'bg-accent-bg' : ''}"
          disabled={option.disabled}
          aria-keyshortcuts={option.key}
          onpointermove={() => {
            if (!option.disabled) pickedId = option.id;
          }}
          onclick={() => onChoose(option.id)}
        >
          <span class="w-[22px] h-[22px] rounded-[7px] flex items-center justify-center font-mono text-[11px] bg-panel-hi text-t2">{option.glyph}</span>
          <span class="flex-1 text-left text-[13.5px] text-t1">{option.label}</span>
          <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] {highlighted ? 'bg-panel' : 'bg-panel-hi'}">{option.key}</span>
        </button>
      {/each}
    </div>
  </div>
</SharedDialog>
