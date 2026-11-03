<script lang="ts">
  import { Tooltip } from "bits-ui";
  import type { ComponentProps } from "svelte";
  import SessionMetaSlot from "../sidebar/SessionMetaSlot.svelte";

  let props: ComponentProps<typeof SessionMetaSlot> = $props();
</script>

<Tooltip.Provider>
  <SessionMetaSlot {...props} />
</Tooltip.Provider>
