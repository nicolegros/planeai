<script lang="ts">
  import { getMinuteNow } from "../../lib/snooze-clock.svelte";
  import { snoozeCountdown, wakeTimeLabel } from "../../lib/task-snooze";

  interface Props {
    /** The wake time, as stored on the task. */
    snoozedUntil: string;
    class?: string;
  }

  let { snoozedUntil, class: className = "" }: Props = $props();

  const until = $derived(Date.parse(snoozedUntil));
  const wakes = $derived(`Wakes ${wakeTimeLabel(until)}`);
</script>

<span class="tabular-nums text-t3 {className}" title={wakes} aria-label={`Snoozed, ${wakes.toLowerCase()}`}>{snoozeCountdown(until, getMinuteNow())}</span>
