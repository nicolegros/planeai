import type { PluginUiEntrypoint } from "../../lib/plugin-sdk";

const STYLES = `
  :host { color: var(--color-t1); font-family: var(--font-sans); }
  .plugin-page { height: 100%; box-sizing: border-box; padding: 32px; background: var(--color-main); }
  .card { max-width: 560px; border: 1px solid var(--color-border); border-radius: 10px; padding: 20px; background: var(--color-panel); }
  h1 { margin: 0 0 8px; font-size: 20px; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 8px 18px; margin: 20px 0 0; }
  dt { color: var(--color-t3); font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; }
  dd { margin: 0; font-family: var(--font-mono); font-size: 13px; }
  .error { color: var(--color-status-exited); }
`;

/** Bundled Jira UI entrypoint. It is intentionally framework-agnostic so its
 * only dependency is the narrow host capability object provided by the SDK. */
export const jiraStatusEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = STYLES;
    const page = document.createElement("main");
    page.className = "plugin-page";
    page.innerHTML = `
      <section class="card" aria-live="polite">
        <h1>Jira</h1>
        <dl>
          <dt>Plugin</dt><dd data-plugin-name></dd>
          <dt>Version</dt><dd data-plugin-version></dd>
          <dt>Host API</dt><dd data-host-api></dd>
          <dt>Status</dt><dd data-status>Loading…</dd>
        </dl>
      </section>`;
    root.replaceChildren(style, page);
    page.querySelector<HTMLElement>("[data-plugin-name]")!.textContent = context.plugin.name;
    page.querySelector<HTMLElement>("[data-plugin-version]")!.textContent = context.plugin.version;
    page.querySelector<HTMLElement>("[data-host-api]")!.textContent =
      context.plugin.host_api_version;
    const status = page.querySelector<HTMLElement>("[data-status]")!;
    let disposed = false;

    void context.host.getJiraStatus(context.plugin.id).then(
      (value) => {
        if (!disposed) status.textContent = value.runtime_state;
      },
      (error: unknown) => {
        if (!disposed) {
          status.textContent = String(error);
          status.classList.add("error");
        }
      },
    );

    return () => {
      disposed = true;
      root.replaceChildren();
    };
  },
};
