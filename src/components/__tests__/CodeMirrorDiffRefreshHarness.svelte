<script lang="ts">
  import CodeMirrorDiff from "../CodeMirrorDiff.svelte";
  import type { TextFileDiff } from "../../lib/review-diff";

  let diff = $state<TextFileDiff>({
    kind: "text",
    original: "original content\n",
    modified: "stale file content\n",
    language: "plaintext",
    original_size: 17,
    modified_size: 19,
  });

  export function refresh(modified: string): void {
    diff = {
      ...diff,
      modified,
      modified_size: modified.length,
    };
  }
</script>

<CodeMirrorDiff
  {diff}
  filePath="example.txt"
  mode="split"
  dark={false}
  fontFamily="monospace"
  fontSize={14}
/>
