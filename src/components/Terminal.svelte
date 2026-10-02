<script lang="ts">
  import { onMount } from "svelte";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getTerminalSettings, isDark } from "../lib/settings.svelte";
  import { extractCommandName } from "../lib/shell-title";
  import { taskWorkspaceLayout } from "../lib/task-workspace-layout.svelte";
  import type { TerminalView } from "../lib/terminal-view";
  import { acquireTerminalView } from "../lib/terminal-views";

  interface Props {
    sessionId: string;
    visible: boolean;
    focused: boolean;
    focusRequest?: { id: number; sessionId: string } | null;
    exited?: boolean;
    skipAttach?: boolean;
    initialCommand?: string;
    onUserInput?: () => void;
    onAttached?: () => void;
    onAttachError?: (error: unknown) => void;
    onFocused?: (event: PointerEvent | FocusEvent) => void;
  }

  let { sessionId, visible, focused, focusRequest = null, exited = false, skipAttach = false, initialCommand, onUserInput, onAttached, onAttachError, onFocused }: Props = $props();

  let containerEl: HTMLDivElement;
  let view = $state<TerminalView | null>(null);
  let handledFocusRequest = 0;
  let suppressProgrammaticFocusin = false;

  function handleTerminalFocus(event: PointerEvent | FocusEvent): void {
    if (event.type === "focusin" && suppressProgrammaticFocusin) return;
    onFocused?.(event);
  }

  // This component is a slot: the view outlives it and is only re-parented here.
  onMount(() => {
    const kind = skipAttach ? "shell" : "agent";
    const acquired = acquireTerminalView({
      ptyKey: sessionId,
      kind,
      initialCommand,
      onTitle: kind === "shell"
        ? (title) => {
            const name = extractCommandName(title);
            if (name) taskWorkspaceLayout.setTabTitle(sessionId, name);
          }
        : undefined,
    });
    acquired.setExited(exited);
    acquired.mount(containerEl);
    view = acquired;
    return () => {
      if (acquired.unmount(containerEl)) acquired.setEvents(null);
      view = null;
    };
  });

  $effect(() => {
    view?.setEvents({
      attached: () => onAttached?.(),
      attachError: (error) => {
        if (onAttachError) onAttachError(error);
        else showSnackbar(String(error));
      },
      userInput: () => onUserInput?.(),
    });
  });

  $effect(() => {
    view?.setExited(exited);
  });

  $effect(() => {
    view?.setShown(visible);
  });

  $effect(() => {
    view?.setFocused(focused);
  });

  $effect(() => {
    const request = focusRequest;
    const current = view;
    if (!current || !request || request.id === handledFocusRequest || request.sessionId !== sessionId || !visible) return;
    handledFocusRequest = request.id;
    // A request can arrive while the font is still loading; apply it once open.
    void current.opened.then(() => requestAnimationFrame(() => {
      if (focusRequest?.id !== request.id || view !== current) return;
      suppressProgrammaticFocusin = true;
      current.focus();
      queueMicrotask(() => { suppressProgrammaticFocusin = false; });
    }));
  });

  // Only re-run when terminal-relevant settings change.
  $effect(() => {
    const current = view;
    if (!current) return;
    const { font_size, font_family, option_as_meta } = getTerminalSettings();
    void [font_size, font_family, option_as_meta];
    isDark();
    current.refreshAppearance();
  });
</script>

<!-- xterm owns its interactive canvas/textarea; this boundary forwards focus to the host. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={containerEl}
  class="w-full h-full"
  class:hidden={!visible}
  onpointerdown={onFocused}
  onfocusin={handleTerminalFocus}
></div>
