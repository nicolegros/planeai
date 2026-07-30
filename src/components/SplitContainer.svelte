<script lang="ts">
  /**
   * SplitContainer — recursively renders the binary split tree.
   * Internal nodes render two children with a divider.
   * Leaf nodes delegate to the parent via the `renderLeaf` snippet.
   */
  import type { TreeNode, SplitNode, LeafNode } from "../lib/split-tree.svelte";
  import { setRatio } from "../lib/split-tree.svelte";
  import SplitDivider from "./SplitDivider.svelte";
  import SplitContainer from "./SplitContainer.svelte";
  import type { Snippet } from "svelte";

  interface Props {
    node: TreeNode;
    renderLeaf: Snippet<[LeafNode]>;
  }

  let { node, renderLeaf }: Props = $props();
</script>

{#if node.type === "leaf"}
  {@render renderLeaf(node)}
{:else}
  {@const split = node as SplitNode}
  <div
    class="split-container"
    class:split-vertical={split.direction === "vertical"}
    class:split-horizontal={split.direction === "horizontal"}
  >
    <div
      class="split-child"
      style={split.direction === "vertical"
        ? `width: ${split.ratio * 100}%`
        : `height: ${split.ratio * 100}%`}
    >
      <SplitContainer node={split.children[0]} {renderLeaf} />
    </div>
    <SplitDivider
      direction={split.direction}
      currentRatio={split.ratio}
      onResize={(ratio) => setRatio(split.id, ratio)}
      onDoubleClick={() => setRatio(split.id, 0.5)}
    />
    <div
      class="split-child"
      style={split.direction === "vertical"
        ? `width: ${(1 - split.ratio) * 100}%`
        : `height: ${(1 - split.ratio) * 100}%`}
    >
      <SplitContainer node={split.children[1]} {renderLeaf} />
    </div>
  </div>
{/if}

<style>
  .split-container {
    display: flex;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .split-vertical {
    flex-direction: row;
  }

  .split-horizontal {
    flex-direction: column;
  }

  .split-child {
    position: relative;
    overflow: hidden;
    min-width: 0;
    min-height: 0;
  }
</style>
