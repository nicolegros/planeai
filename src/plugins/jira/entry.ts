import type { PluginUiContext, PluginUiEntrypoint } from "../../lib/plugin-sdk";

type JiraSource = {
  jql: string;
  status_map?: Record<string, string>;
  writeback?: { on_start?: string | null; on_complete?: string | null; comment?: boolean } | null;
};
type JiraSettings = {
  site: string;
  sync_interval_ms?: number;
  sources?: Record<string, JiraSource>;
};
type JiraStatus = {
  connected: boolean;
  authorizing: boolean;
  site: string | null;
  last_error: string | null;
};
type SyncTotals = { created: number; updated: number; departed: number; errors: number };
type SidebarItem = { key: string; title: string; status: string; child_count: number };
const styles = `:host { color: var(--color-t1); font-family: var(--font-sans); display:block; } .page { max-width:700px; padding:12px 0; } .card { border:1px solid var(--color-border); border-radius:10px; padding:16px; margin-bottom:12px; background:var(--color-panel); } h2 { font-size:13px; margin:0 0 10px; } label { display:block; color:var(--color-t2); font-size:12px; margin:8px 0 4px; } input,select { width:100%; box-sizing:border-box; border:1px solid var(--color-border); border-radius:6px; background:var(--color-main); color:var(--color-t1); padding:7px; } button { border:0; border-radius:6px; padding:7px 10px; background:var(--color-panel-hi); color:var(--color-t1); cursor:pointer; font:inherit; } button.primary { background:var(--color-accent); color:var(--color-on-accent); } button.danger { color:var(--color-status-exited); } button:disabled { opacity:.55; cursor:default; } .row { display:flex; gap:8px; align-items:center; } .row>* { flex:1; } .row button { flex:0 0 auto; } .muted { color:var(--color-t3); font-size:12px; } .error { color:var(--color-status-exited); font-size:12px; } .warning { color:var(--color-status-review); font-size:12px; } .source { border-top:1px solid var(--color-border); margin-top:12px; padding-top:12px; } .sidebar-section { margin-top:8px; } .section-header,.issue { width:100%; display:flex; align-items:center; gap:7px; text-align:left; background:transparent; } .section-header { padding:5px 8px; font-size:11px; font-weight:600; text-transform:uppercase; letter-spacing:.05em; } .issue { padding:6px 8px; font-size:12px; } .issue:hover,.section-header:hover { background:var(--color-panel-hi); } .selected { outline:2px solid var(--color-accent); outline-offset:-2px; } .dot { width:7px; height:7px; border-radius:99px; background:var(--color-t3); flex:0 0 auto; } .dot.active,.dot.done { background:var(--color-status-running); } .key { color:var(--color-t3); font-family:var(--font-mono); font-size:10px; flex:0 0 auto; } .title { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .count { margin-left:auto; color:var(--color-t3); font-size:10px; }`;
function call<T>(context: PluginUiContext, method: string, params: unknown = null): Promise<T> {
  return context.host.call<T>(method, params);
}
function input(
  value: string,
  changed: (value: string) => void,
  placeholder = "",
): HTMLInputElement {
  const element = document.createElement("input");
  element.value = value;
  element.placeholder = placeholder;
  element.addEventListener("change", () => changed(element.value));
  return element;
}
function field(label: string, element: HTMLElement): HTMLElement {
  const wrap = document.createElement("div");
  const heading = document.createElement("label");
  heading.textContent = label;
  wrap.append(heading, element);
  return wrap;
}

export const jiraPreferencesEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = styles;
    const page = document.createElement("section");
    page.className = "page";
    root.replaceChildren(style, page);
    let settings: JiraSettings = { site: "", sync_interval_ms: 60000, sources: {} };
    let status: JiraStatus = { connected: false, authorizing: false, site: null, last_error: null };
    let syncing = false;
    let result: SyncTotals | null = null;
    let disposed = false;
    const save = async (next: JiraSettings) => {
      settings = await call<JiraSettings>(context, "jira.settings.update", next);
      await context.host.data.changed();
      render();
    };
    const refresh = async () => {
      const [nextSettings, nextStatus] = await Promise.all([
        call<Partial<JiraSettings>>(context, "jira.settings.get"),
        call<Partial<JiraStatus>>(context, "jira.status"),
      ]);
      settings = {
        site: nextSettings.site ?? "",
        sync_interval_ms: nextSettings.sync_interval_ms ?? 60000,
        sources: nextSettings.sources ?? {},
      };
      status = {
        connected: nextStatus.connected ?? false,
        authorizing: nextStatus.authorizing ?? false,
        site: nextStatus.site ?? null,
        last_error: nextStatus.last_error ?? null,
      };
      if (!disposed) render();
    };
    const connect = async () => {
      try {
        await save(settings);
        const started = await call<{ authorization_url: string }>(context, "jira.connect.start", {
          attempt_id: crypto.randomUUID(),
        });
        await call(context, "jira.open_browser", { url: started.authorization_url });
        await call(context, "jira.connect.complete", {});
        const poll = async () => {
          status = await call<JiraStatus>(context, "jira.status");
          if (!disposed) render();
          if (status.authorizing && !disposed) setTimeout(() => void poll(), 500);
        };
        await poll();
      } catch (error) {
        status = { ...status, last_error: String(error) };
        render();
      }
    };
    const sync = async () => {
      syncing = true;
      render();
      try {
        result = await call<SyncTotals>(context, "jira.syncNow");
        await context.host.data.changed();
      } catch (error) {
        result = { created: 0, updated: 0, departed: 0, errors: 1 };
        status = { ...status, last_error: String(error) };
      } finally {
        syncing = false;
        render();
      }
    };
    const render = () => {
      const connection = document.createElement("section");
      connection.className = "card";
      connection.innerHTML = `<h2>Jira connection</h2><p class="muted">${status.connected ? `Connected to ${status.site ?? settings.site}` : "Not connected"}</p>`;
      const actions = document.createElement("div");
      actions.className = "row";
      if (status.connected) {
        const disconnect = document.createElement("button");
        disconnect.textContent = "Disconnect";
        disconnect.onclick = async () => {
          await call(context, "jira.disconnect");
          await refresh();
        };
        const now = document.createElement("button");
        now.className = "primary";
        now.textContent = syncing ? "Syncing…" : "Sync Now";
        now.disabled = syncing;
        now.onclick = () => void sync();
        actions.append(disconnect, now);
      } else {
        const connectButton = document.createElement("button");
        connectButton.className = "primary";
        connectButton.textContent = status.authorizing ? "Authorizing…" : "Connect";
        connectButton.disabled = !settings.site.trim() || status.authorizing;
        connectButton.onclick = () => void connect();
        actions.append(connectButton);
      }
      connection.append(actions);
      if (result) {
        const totals = document.createElement("p");
        totals.className = result.errors ? "warning" : "muted";
        totals.textContent = `${result.created} created · ${result.updated} updated · ${result.departed} departed · ${result.errors} error${result.errors === 1 ? "" : "s"}`;
        connection.append(totals);
      }
      if (status.last_error) {
        const error = document.createElement("p");
        error.className = "error";
        error.textContent = status.last_error;
        connection.append(error);
      }
      const config = document.createElement("section");
      config.className = "card";
      config.innerHTML = "<h2>Configuration</h2>";
      config.append(
        field(
          "Site URL",
          input(
            settings.site,
            (site) => void save({ ...settings, site }),
            "https://company.atlassian.net",
          ),
        ),
      );
      const sources = settings.sources ?? {};
      for (const [name, source] of Object.entries(sources)) {
        const card = document.createElement("article");
        card.className = "source";
        const head = document.createElement("div");
        head.className = "row";
        head.append(
          field(
            "Source name",
            input(name, async (nextName) => {
              if (!nextName || nextName === name || sources[nextName]) return;
              await call(context, "jira.sources.rename", { from: name, to: nextName });
              const next = { ...sources, [nextName]: source };
              delete next[name];
              await save({ ...settings, sources: next });
            }),
          ),
        );
        const remove = document.createElement("button");
        remove.className = "danger";
        remove.textContent = "Remove";
        remove.onclick = () => {
          const next = { ...sources };
          delete next[name];
          void save({ ...settings, sources: next });
        };
        head.append(remove);
        card.append(head);
        card.append(
          field(
            "JQL",
            input(
              source.jql ?? "",
              (jql) =>
                void save({ ...settings, sources: { ...sources, [name]: { ...source, jql } } }),
              "project = PROJ",
            ),
          ),
        );
        const map = source.status_map ?? {};
        const mapWrap = document.createElement("div");
        const mapLabel = document.createElement("label");
        mapLabel.textContent = "Status mapping";
        mapWrap.append(mapLabel);
        for (const [jiraStatus, planeaiStatus] of Object.entries(map)) {
          const row = document.createElement("div");
          row.className = "row";
          row.append(
            input(jiraStatus, (nextStatus) => {
              const next = { ...map };
              delete next[jiraStatus];
              if (nextStatus) next[nextStatus] = planeaiStatus;
              void save({
                ...settings,
                sources: { ...sources, [name]: { ...source, status_map: next } },
              });
            }),
          );
          const select = document.createElement("select");
          for (const value of ["todo", "in_progress", "in_review", "done"])
            select.append(
              new Option(value, value, value === planeaiStatus, value === planeaiStatus),
            );
          select.onchange = () =>
            void save({
              ...settings,
              sources: {
                ...sources,
                [name]: { ...source, status_map: { ...map, [jiraStatus]: select.value } },
              },
            });
          row.append(select);
          const removeMap = document.createElement("button");
          removeMap.textContent = "×";
          removeMap.onclick = () => {
            const next = { ...map };
            delete next[jiraStatus];
            void save({
              ...settings,
              sources: { ...sources, [name]: { ...source, status_map: next } },
            });
          };
          row.append(removeMap);
          mapWrap.append(row);
        }
        const addMap = document.createElement("button");
        addMap.textContent = "Add status mapping";
        addMap.onclick = () => {
          let key = "Jira status";
          let i = 2;
          while (map[key]) key = `Jira status ${i++}`;
          void save({
            ...settings,
            sources: { ...sources, [name]: { ...source, status_map: { ...map, [key]: "todo" } } },
          });
        };
        mapWrap.append(addMap);
        card.append(mapWrap);
        const writeback = source.writeback ?? {};
        card.append(
          field(
            "Writeback on_start",
            input(
              writeback.on_start ?? "",
              (on_start) =>
                void save({
                  ...settings,
                  sources: {
                    ...sources,
                    [name]: { ...source, writeback: { ...writeback, on_start: on_start || null } },
                  },
                }),
            ),
          ),
        );
        card.append(
          field(
            "Writeback on_complete",
            input(
              writeback.on_complete ?? "",
              (on_complete) =>
                void save({
                  ...settings,
                  sources: {
                    ...sources,
                    [name]: {
                      ...source,
                      writeback: { ...writeback, on_complete: on_complete || null },
                    },
                  },
                }),
            ),
          ),
        );
        const comment = document.createElement("label");
        const checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.checked = !!writeback.comment;
        checkbox.onchange = () =>
          void save({
            ...settings,
            sources: {
              ...sources,
              [name]: { ...source, writeback: { ...writeback, comment: checkbox.checked } },
            },
          });
        comment.append(checkbox, document.createTextNode(" Add comment on transition"));
        card.append(comment);
        config.append(card);
      }
      const add = document.createElement("button");
      add.textContent = "Add source";
      add.onclick = () => {
        let name = "source";
        let i = 2;
        while (sources[name]) name = `source-${i++}`;
        void save({
          ...settings,
          sources: { ...sources, [name]: { jql: "", status_map: {}, writeback: null } },
        });
      };
      config.append(add);
      page.replaceChildren(connection, config);
    };
    void refresh().catch((error) => {
      status = { ...status, last_error: String(error) };
      render();
    });
    return () => {
      disposed = true;
      root.replaceChildren();
    };
  },
};

export const jiraSidebarSectionEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = styles;
    const section = document.createElement("section");
    section.className = "sidebar-section";
    root.replaceChildren(style, section);
    let items: SidebarItem[] = [];
    let collapsed = false;
    let selected = "";
    let unregister = () => {};
    let disposed = false;
    const render = () => {
      unregister();
      const header = document.createElement("button");
      header.className = `section-header ${selected === "header" ? "selected" : ""}`;
      header.textContent = `${collapsed ? "›" : "⌄"} Jira`;
      const count = document.createElement("span");
      count.className = "count";
      count.textContent = String(items.length);
      header.append(count);
      header.onclick = () => {
        collapsed = !collapsed;
        render();
      };
      section.replaceChildren(header);
      if (!collapsed)
        for (const item of items) {
          const row = document.createElement("button");
          row.className = `issue ${selected === item.key ? "selected" : ""}`;
          row.innerHTML = `<span class="dot ${item.status === "done" ? "done" : item.status !== "todo" ? "active" : ""}"></span><span class="key"></span><span class="title"></span>${item.child_count ? `<span class="count">${item.child_count}</span>` : ""}`;
          row.querySelector(".key")!.textContent = item.key;
          row.querySelector(".title")!.textContent = item.title;
          row.onclick = () => {
            selected = item.key;
            render();
          };
          section.append(row);
        }
      unregister = context.host.sidebar.register([
        {
          id: "header",
          onSelect: () => {
            collapsed = !collapsed;
            render();
          },
          onCollapse: () => {
            collapsed = true;
            render();
          },
          onExpand: () => {
            collapsed = false;
            render();
          },
          onFocus: (active) => {
            if (active) {
              selected = "header";
              render();
              header.scrollIntoView({ block: "nearest" });
            }
          },
        },
        ...(!collapsed
          ? items.map((item) => ({
              id: `issue:${item.key}`,
              onSelect: () => {
                selected = item.key;
                render();
              },
              onCollapse: () => {
                selected = "header";
                render();
              },
              onFocus: (active: boolean) => {
                if (active) {
                  selected = item.key;
                  render();
                }
              },
            }))
          : []),
      ]);
    };
    const refresh = async () => {
      const value = await call<{ items: SidebarItem[] }>(context, "jira.sidebar.items");
      items = value.items;
      if (!disposed) render();
    };
    void refresh().catch(() => {
      if (!disposed) render();
    });
    return () => {
      disposed = true;
      unregister();
      root.replaceChildren();
    };
  },
};
export const jiraStatusEntrypoint: PluginUiEntrypoint = jiraPreferencesEntrypoint;
