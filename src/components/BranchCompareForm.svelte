<script lang="ts">
  import { git, projects as projectsApi } from "../lib/api";
  import type { CommitEntry } from "../lib/types";
  import { Select, Label, Button } from "./ui";
  import FormDialog from "./ui/FormDialog.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";

  interface Props {
    repoPath: string;
    baseBranch: string;
    currentBase: string;
    currentHead: string | null;
    onConfirm: (baseRef: string, headRef: string | null) => void;
    onCancel: () => void;
  }

  let { repoPath, baseBranch, currentBase, currentHead, onConfirm, onCancel }: Props = $props();

  let baseValue = $state(currentBase);
  let headValue = $state(currentHead ?? "__working_tree__");
  let branches = $state<{ value: string; label: string }[]>([]);
  let commits = $state<CommitEntry[]>([]);
  let wrapperEl = $state<HTMLElement | undefined>();

  // Build items for the base picker (branches only)
  const baseItems = $derived(branches);

  // Build items for the head picker (working tree + commits + branches)
  const headItems = $derived([
    { value: "__working_tree__", label: "Working tree" },
    ...commits.map((c) => ({ value: c.sha, label: `${c.short_sha} ${c.subject}` })),
    ...branches.map((b) => ({ ...b, label: `⎇ ${b.label}` })),
  ]);

  const fk = createFormKeyboardController(
    () => [
      { key: "b", ref: () => wrapperEl?.querySelector("[data-field='base'] input") as HTMLElement | null },
      { key: "h", ref: () => wrapperEl?.querySelector("[data-field='head'] input") as HTMLElement | null },
    ],
    { wrapper: () => wrapperEl ?? null, onDismiss: onCancel },
  );

  function confirm() {
    const head = headValue === "__working_tree__" ? null : headValue;
    onConfirm(baseValue, head);
  }

  function handleFormKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) {
      e.preventDefault();
      confirm();
      return;
    }
  }

  // Load branches and commits on mount
  $effect(() => {
    projectsApi.listBranches(repoPath).then(
      (b) => {
        branches = b.map((s) => {
          const remote = s.startsWith("remote:");
          const name = remote ? s.slice(7) : s;
          return { value: remote ? `remote:${name}` : name, label: name };
        });
      },
      () => (branches = []),
    );
    git.listCommits(repoPath, 15).then(
      (c) => (commits = c),
      () => (commits = []),
    );
  });

  // Focus wrapper on mount for normal mode keyboard control (no field focused)
  $effect(() => {
    if (wrapperEl) {
      requestAnimationFrame(() => wrapperEl?.focus());
    }
  });
</script>

<FormDialog title="Compare Branches" onClose={onCancel} preventAutoFocus>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={wrapperEl} tabindex="0" onkeydown={(e) => { handleFormKeydown(e); fk.handleKeydown(e); }} onfocusin={(e) => { if (e.target === wrapperEl) return; fk.handleFocusin(e); }} class="outline-none px-5 pb-5" data-form-keyboard>
    <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); confirm(); }} onkeydown={handleFormKeydown}>
      <div class="space-y-1" data-field="base">
        <Label>Base <span class="font-mono text-[10px] px-1 rounded {fk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">b</span></Label>
        <Select items={baseItems} bind:value={baseValue} placeholder={baseBranch} emptyText="No branches" />
      </div>
      <div class="space-y-1" data-field="head">
        <Label>Head <span class="font-mono text-[10px] px-1 rounded {fk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">h</span></Label>
        <Select items={headItems} bind:value={headValue} placeholder="Working tree" emptyText="No refs" />
      </div>
      <div class="flex items-center justify-between pt-2 border-t border-border">
        <div class="flex items-center gap-2">
          {#if fk.mode === "insert"}
            <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
            <span class="text-[10px] text-t3">esc → normal mode</span>
          {:else}
            <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
            <span class="text-[10px] text-t3">press a key to focus field</span>
          {/if}
        </div>
        <div class="flex gap-2">
          <Button type="button" onclick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary">
            Compare <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>
          </Button>
        </div>
      </div>
    </form>
  </div>
</FormDialog>
