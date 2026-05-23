<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  let projects = $state<Project[]>([]);
  let showProjectForm = $state(false);

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  onMount(() => {
    loadProjects();
    const cleanup = installKeyboardRouter((action) => {
      if (action.type === "new_session") {
        // For now, open project form if no projects exist
        if (projects.length === 0) showProjectForm = true;
      }
    });
    return cleanup;
  });

  const zone = $derived(getActiveZone());
</script>

<main class="flex h-screen">
  <Sidebar
    {projects}
    {zone}
    onAddProject={() => (showProjectForm = true)}
  />

  <section class="flex-1 flex items-center justify-center relative">
    {#if showProjectForm}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-10">
        <ProjectForm
          onCreated={() => { showProjectForm = false; loadProjects(); }}
          onCancel={() => (showProjectForm = false)}
        />
      </div>
    {:else}
      <p class="text-neutral-500">Terminal area</p>
    {/if}
  </section>
</main>
