<script lang="ts">
  import type { ComponentProps } from "svelte";
  import Dialog from "../ui/Dialog.svelte";
  import FormDialog from "../ui/FormDialog.svelte";
  import SessionForm from "../SessionForm.svelte";
  import type { DialogHandoff } from "./dialog-handoff-state.svelte";

  interface Props {
    handoff: DialogHandoff;
    form: ComponentProps<typeof SessionForm>;
  }

  let { handoff, form }: Props = $props();
</script>

<!--
  Mirrors App.svelte: the New Session dialog block is declared before the
  New Item dialog block, so on handoff the new dialog mounts before the old one
  unmounts and runs its close-auto-focus.
-->
{#if handoff.showSession}
  <FormDialog title="New Session" onClose={form.onCancel}>
    <SessionForm {...form} />
  </FormDialog>
{/if}

{#if handoff.showNewItem}
  <Dialog open={true} title="New…" preventOpenAutoFocus={true}>
    <div>
      <button type="button">Session</button>
    </div>
  </Dialog>
{/if}
