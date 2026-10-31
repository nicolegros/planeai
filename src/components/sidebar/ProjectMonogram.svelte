<script lang="ts">
  import { projectHue } from "../../lib/sidebar-cards";
  import type { Project } from "../../lib/types";

  interface Props {
    project: Project;
    class?: string;
  }

  let { project, class: className = "" }: Props = $props();
</script>

<span
  aria-hidden="true"
  class="flex size-4 shrink-0 items-center justify-center rounded-[4px] bg-monogram text-[10px] leading-none font-semibold text-on-monogram {className}"
  style:--monogram-hue={projectHue(project.id)}
>{project.name.trim().charAt(0).toUpperCase()}</span>
