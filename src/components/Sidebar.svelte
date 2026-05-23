<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Props {
    projects: Project[];
    zone: FocusZone;
    onAddProject: () => void;
  }

  let { projects, zone, onAddProject }: Props = $props();
</script>

<aside
  class="w-56 border-r border-neutral-800 flex flex-col {zone === 'sidebar' ? 'bg-neutral-900' : 'bg-neutral-950'}"
>
  <div class="flex items-center justify-between p-3 pb-1">
    <h2 class="text-xs font-semibold text-neutral-500 uppercase tracking-wide">Projects</h2>
    <button
      onclick={onAddProject}
      class="text-neutral-500 hover:text-neutral-300 text-lg leading-none"
      title="Add project"
    >+</button>
  </div>

  <div class="flex-1 overflow-y-auto p-3 pt-1">
    {#if projects.length === 0}
      <div class="mt-8 text-center">
        <p class="text-xs text-neutral-500 mb-2">No projects registered.</p>
        <button
          onclick={onAddProject}
          class="text-xs text-neutral-400 hover:text-neutral-200 underline"
        >
          Add a project
        </button>
      </div>
    {:else}
      {#each projects as project (project.id)}
        <div class="mb-2">
          <span class="text-sm text-neutral-200">{project.name}</span>
          <p class="text-xs text-neutral-500 truncate">{project.path}</p>
        </div>
      {/each}
    {/if}
  </div>
</aside>
