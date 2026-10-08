<script lang="ts" module>
  import type { Provider } from "../../lib/settings.svelte";

  export interface AgentDialogResult {
    name: string;
    provider: Provider;
  }
</script>

<script lang="ts">
  import { Button, Dialog } from "../ui";
  import TextField from "./TextField.svelte";

  interface Props {
    /** Editing an existing agent when set; adding a custom agent otherwise. */
    editing: { key: string; label: string; provider: Provider } | null;
    error: string;
    onSave: (result: AgentDialogResult) => void;
    onClose: () => void;
  }

  let { editing, error, onSave, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(editing?.key ?? "");
  // svelte-ignore state_referenced_locally
  let command = $state(editing?.provider.command ?? "");
  // svelte-ignore state_referenced_locally
  let yoloFlag = $state(editing?.provider.yolo_flag ?? "");
  // svelte-ignore state_referenced_locally
  let resumeCommand = $state(editing?.provider.resume_command ?? "");
  // svelte-ignore state_referenced_locally
  let promptCommand = $state(editing?.provider.prompt_command ?? "");

  const title = $derived(editing ? `Edit ${editing.label}` : "Add custom agent");

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onSave({
      name,
      provider: {
        ...editing?.provider,
        command: command.trim(),
        yolo_flag: yoloFlag.trim() || null,
        resume_command: resumeCommand.trim() || null,
        prompt_command: promptCommand.trim() || null,
      },
    });
  }
</script>

{#snippet field(id: string, label: string, help: string | null, value: string, set: (v: string) => void, placeholder: string)}
  <div class="space-y-1">
    <label class="text-[12px] text-t2" for={id}>{label}</label>
    <TextField {id} {value} oninput={(e) => set(e.currentTarget.value)} {placeholder} mono width="w-full" />
    {#if help}<p class="text-[11px] text-t3">{help}</p>{/if}
  </div>
{/snippet}

<Dialog open={true} onOpenChange={(open) => { if (!open) onClose(); }} {title} class="w-[30rem] p-5">
  <form class="space-y-4" onsubmit={submit}>
    <h2 class="text-[15px] font-semibold text-t1">{title}</h2>
    {#if !editing}
      {@render field("agent-name", "Name", "Shown in session pickers.", name, (v) => (name = v), "e.g. goose")}
    {/if}
    {@render field("agent-command", "Command", null, command, (v) => (command = v), "e.g. goose session")}
    {@render field("agent-yolo", "Skip-permissions flag (optional)", "Appended when a session runs without approval prompts.", yoloFlag, (v) => (yoloFlag = v), "e.g. --yes")}
    {#if editing}
      {@render field("agent-resume", "Resume command (optional)", "Run when restarting an exited session on focus. Falls back to the command when empty.", resumeCommand, (v) => (resumeCommand = v), "e.g. claude --resume")}
      {@render field("agent-prompt", "Prompt arguments (optional)", "{prompt} is replaced with the rendered task prompt.", promptCommand, (v) => (promptCommand = v), "{prompt} or -p {prompt}")}
    {/if}
    {#if error}<p class="text-[12px] text-status-exited" role="alert">{error}</p>{/if}
    <div class="flex justify-end gap-2 pt-1">
      <Button type="button" onclick={onClose}>Cancel</Button>
      <Button type="submit" variant="primary">{editing ? "Save" : "Add agent"}</Button>
    </div>
  </form>
</Dialog>
