<script lang="ts">
  import { Button, Input, Label, SegmentedControl, Select } from "./ui";
  import FormDialog from "./ui/FormDialog.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { customSnoozeTime, type CustomSnooze } from "../lib/task-snooze";

  interface Props {
    title: string;
    /** Snoozes until `at`; false keeps the dialog open, e.g. when the backend refused. */
    onSnooze: (at: Date) => Promise<boolean>;
    onCancel: () => void;
  }

  let { title, onSnooze, onCancel }: Props = $props();

  type Mode = CustomSnooze["mode"];
  type Unit = Extract<CustomSnooze, { mode: "after" }>["unit"];

  const pad = (n: number) => String(n).padStart(2, "0");
  const tomorrow = new Date(Date.now() + 24 * 3_600_000);

  let mode = $state<Mode>("at");
  let date = $state(`${tomorrow.getFullYear()}-${pad(tomorrow.getMonth() + 1)}-${pad(tomorrow.getDate())}`);
  let time = $state("09:00");
  let amount = $state("1");
  let unit = $state<Unit>("hours");
  let error = $state<string | null>(null);
  let wrapperEl = $state<HTMLElement | undefined>();

  const MODES = [
    { value: "at", label: "At a time" },
    { value: "after", label: "After" },
  ] as const;
  const UNITS = [
    { value: "minutes", label: "minutes" },
    { value: "hours", label: "hours" },
    { value: "days", label: "days" },
  ];

  const input = $derived<CustomSnooze>(mode === "at" ? { mode, date, time } : { mode, amount, unit });

  function toggleMode() {
    mode = mode === "at" ? "after" : "at";
    error = null;
  }

  const fk = createFormKeyboardController(
    () => [
      { key: "m", toggle: toggleMode, shiftToggle: toggleMode },
      ...(mode === "at"
      ? [
          { key: "d", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='date'] input") ?? null },
          { key: "t", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='time'] input") ?? null },
        ]
      : [
          { key: "a", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='amount'] input") ?? null },
          { key: "u", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='unit'] input") ?? null },
        ]),
    ],
    // svelte-ignore state_referenced_locally
    { wrapper: () => wrapperEl ?? null, onDismiss: onCancel },
  );

  async function submit() {
    const result = customSnoozeTime(input, new Date());
    if ("error" in result) {
      error = result.error;
      return;
    }
    error = null;
    if (await onSnooze(result.at)) onCancel();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) {
      e.preventDefault();
      void submit();
    }
  }
</script>

{#snippet hint(key: string)}
  <span class="font-mono text-[10px] px-1 rounded {fk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">{key}</span>
{/snippet}

<FormDialog {title} onClose={onCancel}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div bind:this={wrapperEl} tabindex="0" onkeydown={(e) => { handleKeydown(e); fk.handleKeydown(e); }} onfocusin={(e) => { if (e.target === wrapperEl) return; fk.handleFocusin(e); }} class="outline-none px-5 pb-5" data-form-keyboard>
    <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
      <div class="flex items-center gap-2">
        <SegmentedControl name="snooze-mode" label="Snooze" options={MODES} value={mode} onValueChange={(value) => { mode = value; error = null; }} />
        {@render hint("m")}
      </div>
      {#if mode === "at"}
        <div class="flex gap-2">
          <div class="flex-1 space-y-1" data-field="date">
            <Label>Date {@render hint("d")}</Label>
            <Input type="date" bind:value={date} aria-label="Date" oninput={() => (error = null)} />
          </div>
          <div class="w-32 space-y-1" data-field="time">
            <Label>Time {@render hint("t")}</Label>
            <Input type="time" bind:value={time} aria-label="Time" oninput={() => (error = null)} />
          </div>
        </div>
      {:else}
        <div class="flex gap-2">
          <div class="w-24 space-y-1" data-field="amount">
            <Label>Amount {@render hint("a")}</Label>
            <Input inputmode="numeric" bind:value={amount} aria-label="Amount" oninput={() => (error = null)} />
          </div>
          <div class="flex-1 space-y-1" data-field="unit">
            <Label>Unit {@render hint("u")}</Label>
            <Select items={UNITS} value={unit} onValueChange={(value) => { unit = value as Unit; error = null; }} ariaLabel="Unit" />
          </div>
        </div>
      {/if}
      {#if error}<p class="text-[12px] text-status-exited" role="alert">{error}</p>{/if}
      <div class="flex justify-end gap-2 pt-2 border-t border-border">
        <Button type="button" onclick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary">
          Snooze <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>
        </Button>
      </div>
    </form>
  </div>
</FormDialog>
