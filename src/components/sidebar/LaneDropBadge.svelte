<script lang="ts">
  /** What dropping the dragged task on this lane would do, shown in the lane's header while it hovers. */
  import type { DropVerdict, LaneOutcome } from "../../lib/task-drop";

  let { verdict }: { verdict: DropVerdict<LaneOutcome> } = $props();
</script>

<span
  role="status"
  data-lane-drop={verdict.accepts ? "accepts" : "refuses"}
  class="pointer-events-none inline-flex h-5 max-w-[60%] shrink-0 items-center rounded-sm border px-1.5 text-[10px] font-medium normal-case tracking-normal {verdict.accepts ? 'border-accent/40 bg-accent-bg text-accent' : 'border-status-exited/40 bg-status-exited/10 text-status-exited'}"
><span class="truncate">{verdict.accepts ? verdict.label : verdict.reason}</span></span>
