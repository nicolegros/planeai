<script lang="ts">
  import { locateProjectFolder } from "../lib/locate-project";
  import type { Project } from "../lib/types";

  let { project, onLocated }: { project: Project; onLocated?: (project: Project) => void } = $props();

  let locating = $state(false);

  async function locate() {
    locating = true;
    try {
      const located = await locateProjectFolder(project);
      if (located) onLocated?.(located);
    } finally {
      locating = false;
    }
  }
</script>

<p class="text-xs text-status-review" data-testid="project-folder-missing">
  Project folder not found.
  <button
    type="button"
    class="underline underline-offset-2 hover:text-t1 disabled:opacity-40"
    disabled={locating}
    onclick={locate}
  >Locate folder…</button>
</p>
