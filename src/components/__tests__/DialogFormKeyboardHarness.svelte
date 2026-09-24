<script lang="ts">
  import Dialog from "../ui/Dialog.svelte";

  interface Props {
    preventOpenAutoFocus?: boolean;
  }

  let { preventOpenAutoFocus = false }: Props = $props();
</script>

<Dialog open={true} title="Harness" {preventOpenAutoFocus}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div tabindex="-1" data-form-keyboard data-testid="fk-wrapper">
    <input data-testid="first-field" />
    <button type="button">Submit</button>
  </div>
</Dialog>
