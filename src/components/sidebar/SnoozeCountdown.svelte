<script lang="ts">
  import { getMinuteNow } from "../../lib/snooze-clock.svelte";
  import { snoozeCountdown, wakeTimeLabel } from "../../lib/task-snooze";

  interface Props {
    /** The wake time, as stored on the task; nothing shows without one. */
    snoozedUntil: string | null;
    class?: string;
  }

  let { snoozedUntil, class: className = "" }: Props = $props();

  const until = $derived(snoozedUntil === null ? null : Date.parse(snoozedUntil));
</script>

{#if until !== null}
  {@const wakeTime = wakeTimeLabel(until)}
  <span class="tabular-nums text-t3 {className}" title={`Wakes ${wakeTime}`} aria-label={`Snoozed, wakes ${wakeTime}`}>{snoozeCountdown(until, getMinuteNow())}</span>
{/if}
