<script lang="ts">
  import Dialog from "../ui/Dialog.svelte";

  interface Props {
    preventOpenAutoFocus?: boolean;
    open?: boolean;
  }

  let { preventOpenAutoFocus = false, open = true }: Props = $props();
</script>

<Dialog {open} title="Harness" {preventOpenAutoFocus} initialFocusSelector="[data-form-keyboard]">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div tabindex="-1" data-form-keyboard data-testid="fk-wrapper">
    <input data-testid="first-field" />
    <button type="button">Submit</button>
  </div>
</Dialog>
