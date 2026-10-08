<script lang="ts">
  import type { ComponentProps } from "svelte";
  import FormDialog from "../ui/FormDialog.svelte";
  import TaskForm from "../TaskForm.svelte";

  let props: ComponentProps<typeof TaskForm> = $props();
</script>

<FormDialog title="New Task" onClose={props.onCancel}>
  <TaskForm {...props} />
</FormDialog>
