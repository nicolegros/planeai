<script lang="ts">
  import FileExplorer from "../FileExplorer.svelte";

  let rootPath = $state("/tmp/project-a");
  let sessionId = $state("session-a");

  export function switchSession(nextSessionId: string, nextRootPath: string) {
    sessionId = nextSessionId;
    rootPath = nextRootPath;
  }
</script>

<FileExplorer
  {rootPath}
  {sessionId}
  visible={true}
  onFocus={() => {}}
  onOpenFile={() => {}}
  onPinFile={() => {}}
/>
