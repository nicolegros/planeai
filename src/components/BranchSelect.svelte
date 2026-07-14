<script lang="ts">
  import { projects as projectsApi } from "../lib/api";
  import { Select } from "./ui";

  interface Props {
    projectPath: string;
    value?: string;
    onValueChange?: (branch: string) => void;
    onkeydown?: (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => void;
    placeholder?: string;
    emptyText?: string;
  }

  let { projectPath, value = $bindable(""), onValueChange, onkeydown, placeholder = "Select branch…", emptyText = "No branches found" }: Props = $props();

  let branches = $state<{ value: string; label: string; remote?: boolean }[]>([]);
  let loaded = $state(false);

  $effect(() => {
    if (!projectPath) { branches = []; loaded = true; return; }
    loaded = false;
    projectsApi.listBranches(projectPath).then(
      (b) => {
        branches = b.map((s) => {
          const remote = s.startsWith("remote:");
          const name = remote ? s.slice(7) : s;
          return { value: remote ? `remote:${name}` : name, label: name, remote };
        });
        if (branches.length === 0) {
          console.error("[BranchSelect] No branches found for project:", projectPath);
        }
        loaded = true;
      },
      (err) => {
        console.error("[BranchSelect] Failed to load branches for project:", projectPath, err);
        branches = [];
        loaded = true;
      },
    );
  });
</script>

{#if loaded}
  <Select
    items={branches}
    bind:value
    {onValueChange}
    {onkeydown}
    {placeholder}
    {emptyText}
  />
{:else}
  <input
    disabled
    class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t3 placeholder:text-t3"
    placeholder="Loading branches…"
  />
{/if}
