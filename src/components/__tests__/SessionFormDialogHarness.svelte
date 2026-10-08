<script lang="ts">
  import type { ComponentProps } from "svelte";
  import FormDialog from "../ui/FormDialog.svelte";
  import SessionForm from "../SessionForm.svelte";

  let props: ComponentProps<typeof SessionForm> = $props();
</script>

<FormDialog title="New Session" onClose={props.onCancel}>
  <SessionForm {...props} />
</FormDialog>
