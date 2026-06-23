<script lang="ts">
  interface Props {
    side: "left" | "right";
    width: number;
    min: number;
    max: number;
    defaultWidth: number;
    onResizeEnd?: (width: number) => void;
  }

  let { side, width = $bindable(), min, max, defaultWidth, onResizeEnd }: Props = $props();

  let dragging = $state(false);
  let startX = 0;
  let startWidth = 0;

  function onPointerDown(e: PointerEvent) {
    e.preventDefault();
    dragging = true;
    startX = e.clientX;
    startWidth = width;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    const delta = side === "right" ? e.clientX - startX : startX - e.clientX;
    width = Math.min(max, Math.max(min, startWidth + delta));
  }

  function onPointerUp() {
    if (!dragging) return;
    dragging = false;
    onResizeEnd?.(width);
  }

  function onDblClick() {
    width = defaultWidth;
    onResizeEnd?.(defaultWidth);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="absolute top-0 bottom-0 w-1 cursor-col-resize z-10 transition-colors {side === 'right' ? 'right-0' : 'left-0'} {dragging ? 'bg-accent' : 'hover:bg-accent/50'}"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  ondblclick={onDblClick}
></div>
