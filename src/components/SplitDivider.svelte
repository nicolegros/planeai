<script lang="ts">
  /**
   * SplitDivider — draggable resize handle between split panes.
   * onResize always receives an absolute ratio [0.1, 0.9].
   */
  interface Props {
    direction: "horizontal" | "vertical";
    currentRatio: number;
    onResize: (ratio: number) => void;
    onDoubleClick: () => void;
  }

  let { direction, currentRatio, onResize, onDoubleClick }: Props = $props();

  let dragging = $state(false);
  let parentEl: HTMLElement | null = null;

  function handlePointerDown(e: PointerEvent) {
    e.preventDefault();
    dragging = true;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    parentEl = target.parentElement;
  }

  function handlePointerMove(e: PointerEvent) {
    if (!dragging || !parentEl) return;
    const rect = parentEl.getBoundingClientRect();
    let ratio: number;
    if (direction === "vertical") {
      ratio = (e.clientX - rect.left) / rect.width;
    } else {
      ratio = (e.clientY - rect.top) / rect.height;
    }
    ratio = Math.max(0.1, Math.min(0.9, ratio));
    onResize(ratio);
  }

  function handlePointerUp(e: PointerEvent) {
    dragging = false;
    const target = e.currentTarget as HTMLElement;
    target.releasePointerCapture(e.pointerId);
    parentEl = null;
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="split-divider {direction === 'vertical' ? 'split-divider-vertical' : 'split-divider-horizontal'} {dragging ? 'dragging' : ''}"
  role="separator"
  aria-orientation={direction === "vertical" ? "vertical" : "horizontal"}
  tabindex="0"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  ondblclick={onDoubleClick}
  onkeydown={(e) => {
    // Keyboard resize: arrow keys adjust by 5%
    const step = 0.05;
    if (direction === "vertical") {
      if (e.key === "ArrowLeft") { e.preventDefault(); onResize(Math.max(0.1, currentRatio - step)); }
      if (e.key === "ArrowRight") { e.preventDefault(); onResize(Math.min(0.9, currentRatio + step)); }
    } else {
      if (e.key === "ArrowUp") { e.preventDefault(); onResize(Math.max(0.1, currentRatio - step)); }
      if (e.key === "ArrowDown") { e.preventDefault(); onResize(Math.min(0.9, currentRatio + step)); }
    }
  }}
>
  <div class="split-divider-line"></div>
</div>

<style>
  .split-divider {
    position: relative;
    z-index: 10;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .split-divider-vertical {
    width: 6px;
    cursor: col-resize;
    margin: 0 -3px;
  }

  .split-divider-horizontal {
    height: 6px;
    cursor: row-resize;
    margin: -3px 0;
  }

  .split-divider-line {
    border-radius: 1px;
    background: var(--color-border);
    transition: background 0.15s;
  }

  .split-divider-vertical .split-divider-line {
    width: 2px;
    height: 100%;
  }

  .split-divider-horizontal .split-divider-line {
    width: 100%;
    height: 2px;
  }

  .split-divider:hover .split-divider-line,
  .split-divider.dragging .split-divider-line {
    background: var(--color-accent);
  }

  .split-divider:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
    border-radius: 2px;
  }
</style>
