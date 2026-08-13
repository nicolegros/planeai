import type { PluginUiEntrypoint } from "../../lib/plugin-sdk";

interface JiraConnectionStatus {
  connected: boolean;
  site: string | null;
  runtime_state?: string;
}

const STYLES = `
  :host { color: var(--color-t1); font-family: var(--font-sans); }
  .plugin-page { height: 100%; box-sizing: border-box; padding: 32px; background: var(--color-main); }
  .card { max-width: 560px; border: 1px solid var(--color-border); border-radius: 10px; padding: 20px; background: var(--color-panel); }
  .sidebar { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 8px; background: var(--color-panel); font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 999px; background: var(--color-surface-400); }
  .dot.connected { background: var(--color-status-running); }
  button { margin-left: auto; border: 0; background: transparent; color: var(--color-accent); cursor: pointer; font: inherit; }
  .error { color: var(--color-status-exited); }
`;

function connectionStatus(context: Parameters<PluginUiEntrypoint["mount"]>[1]): Promise<JiraConnectionStatus> {
  return context.host.call<JiraConnectionStatus>("jira.status");
}

export const jiraStatusEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = STYLES;
    const page = document.createElement("main");
    page.className = "plugin-page";
    page.innerHTML = `<section class="card" aria-live="polite"><h1 data-name></h1><p data-meta></p><p data-status>Loading connection status…</p><button type="button">Open Preferences</button></section>`;
    page.querySelector<HTMLElement>("[data-name]")!.textContent = context.plugin.name;
    const meta = page.querySelector<HTMLElement>("[data-meta]")!;
    const status = page.querySelector<HTMLElement>("[data-status]")!;
    page.querySelector("button")!.addEventListener("click", () => context.host.navigation.openPreferences());
    root.replaceChildren(style, page);
    let disposed = false;
    void connectionStatus(context).then(
      (value) => {
        if (!disposed) {
          meta.textContent = `Host API ${context.plugin.host_api_version} · ${value.runtime_state ?? "running"}`;
          status.textContent = value.connected ? `Connected to ${value.site ?? "Jira"}` : "Not connected";
        }
      },
      (error: unknown) => { if (!disposed) { status.textContent = String(error); status.classList.add("error"); } },
    );
    return () => { disposed = true; root.replaceChildren(); };
  },
};

export const jiraConnectionEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = STYLES;
    const section = document.createElement("section");
    section.className = "sidebar";
    section.setAttribute("aria-live", "polite");
    section.innerHTML = `<span class="dot"></span><span data-status>Jira</span><button type="button">Settings</button>`;
    const dot = section.querySelector<HTMLElement>(".dot")!;
    const status = section.querySelector<HTMLElement>("[data-status]")!;
    section.querySelector("button")!.addEventListener("click", () => context.host.navigation.openPreferences());
    root.replaceChildren(style, section);
    let disposed = false;
    void connectionStatus(context).then(
      (value) => {
        if (disposed) return;
        dot.classList.toggle("connected", value.connected);
        status.textContent = value.connected ? `Jira · ${value.site ?? "Connected"}` : "Jira · Not connected";
      },
      (error: unknown) => { if (!disposed) { status.textContent = `Jira · ${String(error)}`; status.classList.add("error"); } },
    );
    return () => { disposed = true; root.replaceChildren(); };
  },
};
