<script lang="ts">
  import type { ComponentProps } from "svelte";
  import type { Project } from "../../lib/types";
  import TaskForm from "../TaskForm.svelte";

  let { projects: initialProjects, ...rest }: ComponentProps<typeof TaskForm> = $props();
  // svelte-ignore state_referenced_locally
  let projects = $state(initialProjects);

  export function setProjects(next: Project[]) {
    projects = next;
  }
</script>

<TaskForm {...rest} {projects} />
