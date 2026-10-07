<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { renderMarkdown } from "../lib/markdown";
  import { showSnackbar } from "../lib/snackbar.svelte";

  interface Props {
    source: string;
    class?: string;
  }

  let { source, class: className = "" }: Props = $props();

  const html = $derived(renderMarkdown(source));
  const OPENABLE_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);

  function onClick(event: MouseEvent): void {
    const target = event.target;
    const anchor = target instanceof Element ? target.closest("a") : null;
    if (!anchor) return;
    // The webview must never navigate away from the app.
    event.preventDefault();
    let url: URL;
    try {
      url = new URL(anchor.getAttribute("href") ?? "");
    } catch {
      return;
    }
    if (!OPENABLE_PROTOCOLS.has(url.protocol)) return;
    void openUrl(url.toString()).catch((error) => showSnackbar(`Failed to open URL: ${String(error)}`));
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="markdown-view text-sm leading-6 text-t2 {className}" onclick={onClick} data-markdown-view>
  {@html html}
</div>

<style>
  .markdown-view {
    overflow-wrap: anywhere;
  }
  .markdown-view :global(> :first-child) {
    margin-top: 0;
  }
  .markdown-view :global(> :last-child) {
    margin-bottom: 0;
  }
  .markdown-view :global(:where(p, ul, ol, blockquote, pre, table, hr)) {
    margin: 0.5rem 0;
  }
  .markdown-view :global(:where(pre, table)) {
    margin: 0.75rem 0;
  }
  .markdown-view :global(:where(h1, h2, h3, h4, h5, h6)) {
    margin: 1rem 0 0.375rem;
    font-weight: 600;
    line-height: 1.375;
    color: var(--color-t1);
  }
  .markdown-view :global(h1) {
    font-size: 1.125rem;
  }
  .markdown-view :global(h2) {
    font-size: 1rem;
  }
  .markdown-view :global(:where(h3, h4, h5, h6)) {
    font-size: 0.875rem;
  }
  .markdown-view :global(ul) {
    list-style: disc;
    padding-left: 1.25rem;
  }
  .markdown-view :global(ol) {
    list-style: decimal;
    padding-left: 1.25rem;
  }
  .markdown-view :global(li > :where(ul, ol)) {
    margin: 0.125rem 0;
  }
  .markdown-view :global(li::marker) {
    color: var(--color-t3);
  }
  .markdown-view :global(blockquote) {
    border-left: 2px solid var(--color-border-s);
    padding-left: 0.75rem;
    color: var(--color-t3);
  }
  .markdown-view :global(code) {
    border-radius: 0.25rem;
    background: var(--color-panel-hi);
    padding: 0.0625rem 0.25rem;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    color: var(--color-t1);
  }
  /* No horizontal scrolling: WebKit's classic scrollbar on overflow:auto overlaps the next block. */
  .markdown-view :global(pre) {
    white-space: pre-wrap;
    border: 1px solid var(--color-border);
    border-radius: 0.375rem;
    background: var(--color-panel-hi);
    padding: 0.5rem 0.75rem;
    line-height: 1.25rem;
  }
  .markdown-view :global(pre code) {
    background: none;
    padding: 0;
    font-size: 0.75rem;
  }
  .markdown-view :global(table) {
    border-collapse: collapse;
  }
  .markdown-view :global(:where(th, td)) {
    border: 1px solid var(--color-border-s);
    padding: 0.25rem 0.5rem;
    text-align: left;
  }
  .markdown-view :global(th) {
    font-weight: 600;
    color: var(--color-t1);
  }
  .markdown-view :global(hr) {
    border: 0;
    border-top: 1px solid var(--color-border-s);
  }
  .markdown-view :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
    text-decoration-color: var(--color-t3);
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .markdown-view :global(a:hover) {
    text-decoration-color: currentColor;
  }
  .markdown-view :global(strong) {
    font-weight: 600;
    color: var(--color-t1);
  }
  .markdown-view :global(em) {
    font-style: italic;
  }
  .markdown-view :global(del) {
    text-decoration: line-through;
  }
</style>
