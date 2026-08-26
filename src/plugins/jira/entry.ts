import { createFormKeyboardController } from "../../lib/form-keyboard.svelte";
import { MOD_ENTER_HINT } from "../../lib/keyboard";
import type {
  PluginModalControls,
  PluginUiContext,
  PluginUiEntrypoint,
} from "../../lib/plugin-sdk";
import { createSearchableCombobox } from "./searchable-combobox";

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
type DepartedItem = { key: string; summary: string };
const styles = `:host { color: var(--color-t1); font-family: var(--font-sans); display:block; } .page { max-width:700px; padding:12px 0; } .card { border:1px solid var(--color-border); border-radius:10px; padding:16px; margin-bottom:12px; background:var(--color-panel); } h2 { font-size:13px; margin:0 0 10px; } label { display:block; color:var(--color-t2); font-size:12px; margin:8px 0 4px; } input,select { width:100%; box-sizing:border-box; border:1px solid var(--color-border); border-radius:6px; background:var(--color-main); color:var(--color-t1); padding:7px; } button { border:0; border-radius:6px; padding:7px 10px; background:var(--color-panel-hi); color:var(--color-t1); cursor:pointer; font:inherit; } button.primary { background:var(--color-accent); color:var(--color-on-accent); } button.danger { color:var(--color-status-exited); } button:disabled { opacity:.55; cursor:default; } .row { display:flex; gap:8px; align-items:center; } .row>* { flex:1; } .row button { flex:0 0 auto; } .muted { color:var(--color-t3); font-size:12px; } .error { color:var(--color-status-exited); font-size:12px; } .warning { color:var(--color-status-review); font-size:12px; } .source { border-top:1px solid var(--color-border); margin-top:12px; padding-top:12px; } .sidebar-section { margin-top:8px; } .section-header,.issue { width:100%; display:flex; align-items:center; gap:7px; text-align:left; background:transparent; } .section-header { padding:5px 8px; font-size:11px; font-weight:600; text-transform:uppercase; letter-spacing:.05em; } .issue { padding:6px 8px; font-size:12px; } .issue:hover,.section-header:hover { background:var(--color-panel-hi); } .section-header:focus,.issue:focus { outline:none; } .section-header.selected,.issue.selected,.section-header:focus-visible,.issue:focus-visible { outline:2px solid var(--color-accent); outline-offset:-2px; } .dot { width:7px; height:7px; border-radius:99px; background:var(--color-t3); flex:0 0 auto; } .dot.active,.dot.done { background:var(--color-status-running); } .key { color:var(--color-t3); font-family:var(--font-mono); font-size:10px; flex:0 0 auto; } .title { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .interaction { position:fixed; bottom:16px; left:16px; pointer-events:auto; max-width:32rem; border-radius:8px; background:var(--color-status-review); color:var(--color-main); padding:12px 16px; box-shadow:0 8px 24px rgba(0,0,0,.3); outline:none; } .interaction .summary { color:color-mix(in srgb, var(--color-main) 80%, transparent); font-size:12px; margin-top:3px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .interaction .actions { margin-top:8px; display:flex; gap:8px; } .interaction button { background:color-mix(in srgb, var(--color-main) 18%, transparent); color:var(--color-main); } .count { margin-left:auto; color:var(--color-t3); font-size:10px; }`;
function call<T>(context: PluginUiContext, method: string, params: unknown = null): Promise<T> {
  return context.host.call<T>(method, params);
}
type InputOptions = { disabled?: boolean; ariaLabel?: string };
let nextFieldId = 0;
function input(
  value: string,
  changed: (value: string) => void,
  placeholder = "",
  options: InputOptions = {},
): HTMLInputElement {
  const element = document.createElement("input");
  element.value = value;
  element.placeholder = placeholder;
  element.disabled = options.disabled ?? false;
  if (options.ariaLabel) element.setAttribute("aria-label", options.ariaLabel);
  element.addEventListener("change", () => changed(element.value));
  return element;
}
function field(label: string, element: HTMLElement): HTMLElement {
  const wrap = document.createElement("div");
  const heading = document.createElement("label");
  const control = element.querySelector<HTMLElement>("[data-field-control]") ?? element;
  const id = control.id || `jira-field-${++nextFieldId}`;
  control.id = id;
  heading.htmlFor = id;
  heading.textContent = label;
  wrap.append(heading, element);
  return wrap;
}

function sidebarStatusLabel(status: string): string {
  return (
    {
      todo: "To do",
      in_progress: "In progress",
      in_review: "In review",
      done: "Done",
    }[status] ?? status
  );
}

type JiraIssue = { key: string; title: string; description: string };
type AssignmentProject = { id: string; name: string; path: string; hidden: boolean };

function openAssignment(context: PluginUiContext, key: string): PluginModalControls {
  const { interaction, projects: projectApi, tasks } = context.host;
  const refreshAssignment = context.host.data.refreshAssignment;
  if (!interaction || !projectApi || !tasks || !refreshAssignment) {
    throw new Error("Jira assignment requires trusted host capabilities.");
  }
  return interaction.openModal({
    title: "Assign Jira issue",
    contentResponsive: true,
    mount(root, controls) {
      const style = document.createElement("style");
      style.textContent = `${styles} .assignment { padding:0 20px 20px; display:grid; gap:14px; } .preview { border:1px solid var(--color-border); border-radius:8px; padding:10px; } .preview h3 { margin:4px 0; font-size:14px; } .preview p { white-space:pre-wrap; margin:0; color:var(--color-t2); font-size:12px; } .actions { display:flex; justify-content:space-between; align-items:center; gap:8px; } .keyboard-mode { display:flex; align-items:center; gap:8px; font-size:10px; color:var(--color-t3); } .keyboard-mode strong { font-family:var(--font-mono); font-size:10px; padding:2px 6px; border-radius:4px; background:var(--color-panel-hi); color:var(--color-t2); } .keyboard-mode.insert strong { background:var(--color-accent-bg); color:var(--color-accent); } .submit-hint { margin-left:4px; font-family:var(--font-mono); font-size:10px; opacity:.6; } .empty { color:var(--color-t2); font-size:12px; }`;
      const body = document.createElement("div");
      let issue: JiraIssue | null = null;
      let projects: AssignmentProject[] = [];
      let selectedProjectId = "";
      let error = "";
      let loading = true;
      let submitting = false;
      let disposed = false;
      body.tabIndex = -1;
      body.dataset.formKeyboard = "";

      const updateKeyboardMode = () => {
        const status = body.querySelector<HTMLElement>("[data-assignment-keyboard-mode]");
        if (!status) return;
        const insert = formKeyboard.mode === "insert";
        status.classList.toggle("insert", insert);
        status.innerHTML = insert
          ? "<strong>INSERT</strong><span>esc → normal mode</span>"
          : "<strong>NORMAL</strong><span>press a key to focus field</span>";
      };
      const formKeyboard = createFormKeyboardController(
        () => [
          {
            key: "p",
            ref: () => body.querySelector<HTMLElement>("[data-jira-project-combobox]") ?? null,
          },
          {
            key: "n",
            toggle: () => {
              if (!submitting) void createProject();
            },
          },
        ],
        { wrapper: () => body, onDismiss: () => controls.close() },
      );
      body.addEventListener("focusin", (event) => {
        formKeyboard.handleFocusin(event);
        updateKeyboardMode();
      });
      body.addEventListener("keydown", (event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          event.stopPropagation();
          void submit();
          return;
        }
        const consumeNormalModeKey =
          formKeyboard.mode === "normal" &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.altKey &&
          event.key !== "Tab";
        formKeyboard.handleKeydown(event);
        if (consumeNormalModeKey) {
          event.stopPropagation();
          if (event.key !== "Enter" && event.key !== " ") event.preventDefault();
        }
        updateKeyboardMode();
      });

      const selectedProject = () =>
        projects.find((project) => project.id === selectedProjectId) ?? null;
      const focusPicker = () =>
        queueMicrotask(() => {
          const picker = root.querySelector<HTMLInputElement>("[data-jira-project-combobox]");
          if (!picker) return;
          picker.dataset.preserveSelectedValueOnFocus = "";
          picker.focus();
        });
      const refreshProjects = async (projectId?: string) => {
        projects = await projectApi.list();
        selectedProjectId = projectId ?? selectedProjectId;
        if (!selectedProject()) selectedProjectId = "";
      };
      const render = () => {
        const form = document.createElement("form");
        form.className = "assignment";
        form.setAttribute("aria-label", "Assign Jira issue to PlaneAI project");
        form.addEventListener("submit", (event) => {
          event.preventDefault();
          void submit();
        });
        if (loading) {
          const message = document.createElement("p");
          message.className = "muted";
          message.textContent = "Loading Jira issue…";
          form.append(message);
        } else if (!issue) {
          const message = document.createElement("p");
          message.className = "error";
          message.setAttribute("role", "alert");
          message.textContent = error || "This Jira issue is no longer available.";
          form.append(message);
        } else {
          const preview = document.createElement("section");
          preview.className = "preview";
          const issueKey = document.createElement("span");
          issueKey.className = "key";
          issueKey.textContent = issue.key;
          const title = document.createElement("h3");
          title.textContent = issue.title;
          const description = document.createElement("p");
          description.textContent = issue.description || "No description";
          preview.append(issueKey, title, description);
          form.append(preview);

          if (projects.length === 0) {
            const empty = document.createElement("p");
            empty.className = "empty";
            empty.textContent = "No PlaneAI projects available. Press N to create one.";
            form.append(empty);
          } else {
            const picker = createSearchableCombobox({
              ariaLabel: "PlaneAI project",
              items: projects.map((project) => ({ value: project.id, label: project.name })),
              value: selectedProjectId,
              disabled: submitting,
              placeholder: "Search projects…",
              emptyText: "No projects found",
              onValueChange: (projectId) => {
                selectedProjectId = projectId;
                error = "";
                render();
                focusPicker();
              },
            });
            form.append(field("PlaneAI project (P)", picker));
          }
          if (error) {
            const message = document.createElement("p");
            message.className = "error";
            message.setAttribute("role", "alert");
            message.textContent = error;
            form.append(message);
          }
          const actions = document.createElement("div");
          actions.className = "actions";
          const keyboardMode = document.createElement("div");
          keyboardMode.className = "keyboard-mode";
          keyboardMode.dataset.assignmentKeyboardMode = "";
          actions.append(keyboardMode);
          const actionButtons = document.createElement("div");
          actionButtons.className = "row";
          const newProject = document.createElement("button");
          newProject.type = "button";
          newProject.textContent = "New Project (N)";
          newProject.disabled = submitting;
          newProject.onclick = () => void createProject();
          const assign = document.createElement("button");
          assign.type = "submit";
          assign.className = "primary";
          if (submitting) {
            assign.textContent = "Assigning…";
          } else {
            assign.append("Assign ");
            const submitHint = document.createElement("span");
            submitHint.className = "submit-hint";
            submitHint.textContent = MOD_ENTER_HINT;
            assign.append(submitHint);
          }
          assign.disabled = submitting || !selectedProject();
          assign.title = MOD_ENTER_HINT;
          actionButtons.append(newProject, assign);
          actions.append(actionButtons);
          form.append(actions);
        }
        body.replaceChildren(form);
        updateKeyboardMode();
      };
      const createProject = async () => {
        const project = await interaction.openProjectForm();
        if (!project || disposed) return;
        try {
          await refreshProjects(project.id);
          error = "";
          render();
          focusPicker();
        } catch (nextError) {
          error = String(nextError);
          render();
        }
      };
      const submit = async () => {
        if (submitting || !issue) return;
        const project = selectedProject();
        if (!project) {
          error = "Choose a PlaneAI project.";
          render();
          focusPicker();
          return;
        }
        submitting = true;
        error = "";
        controls.setSubmitting(true);
        render();
        try {
          await tasks.createChild({
            project,
            title: issue.title,
            description: issue.description,
            parentKey: issue.key,
          });
          await refreshAssignment(project);
          controls.setSubmitting(false);
          controls.close();
        } catch (nextError) {
          error = String(nextError);
          submitting = false;
          controls.setSubmitting(false);
          render();
        }
      };
      root.append(style, body);
      void Promise.all([call<JiraIssue>(context, "jira.issue.get", { key }), refreshProjects()])
        .then(([nextIssue]) => {
          issue = nextIssue;
          loading = false;
          if (!disposed) {
            render();
            queueMicrotask(() => body.focus());
          }
        })
        .catch((nextError) => {
          loading = false;
          error = String(nextError);
          if (!disposed) render();
        });
      render();
      return () => {
        disposed = true;
        root.replaceChildren();
      };
    },
  });
}

export const jiraDepartedInteractionEntrypoint: PluginUiEntrypoint = {
  mount(root, context) {
    const style = document.createElement("style");
    style.textContent = styles;
    const interaction = document.createElement("section");
    interaction.className = "interaction";
    interaction.dataset.pluginInteraction = "";
    interaction.tabIndex = -1;
    interaction.setAttribute("role", "status");
    interaction.setAttribute("aria-live", "polite");
    root.replaceChildren(style, interaction);
    let items: DepartedItem[] = [];
    let disposed = false;
    let resolving = false;
    const render = () => {
      const item = items[0];
      if (!item) {
        interaction.replaceChildren();
        return;
      }
      const title = document.createElement("div");
      title.textContent = `Issue left Jira query — ${item.key}`;
      const summary = document.createElement("div");
      summary.className = "summary";
      summary.textContent = item.summary;
      const actions = document.createElement("div");
      actions.className = "actions";
      const done = document.createElement("button");
      done.type = "button";
      done.textContent = resolving ? "Marking done…" : "Done (D)";
      done.disabled = resolving;
      done.onclick = () => void resolve();
      const dismiss = document.createElement("button");
      dismiss.type = "button";
      dismiss.textContent = "Dismiss (N)";
      dismiss.disabled = resolving;
      dismiss.onclick = () => void dequeue();
      actions.append(done, dismiss);
      interaction.replaceChildren(title, summary, actions);
    };
    const refresh = async () => {
      const value = await call<{ items: DepartedItem[] }>(context, "jira.departures.list");
      items = value.items;
      if (!disposed) render();
    };
    const dequeue = async () => {
      const item = items[0];
      if (!item || resolving) return;
      await call(context, "jira.departures.dequeue", { key: item.key });
      await refresh();
    };
    const resolve = async () => {
      const item = items[0];
      if (!item || resolving) return;
      resolving = true;
      render();
      try {
        // The sidecar dequeues only after its host task update succeeds.
        await call(context, "jira.departures.resolve", { key: item.key });
        await refresh();
      } catch (error) {
        context.host.data.notify(`Could not mark ${item.key} done: ${String(error)}`);
      } finally {
        resolving = false;
        if (!disposed) render();
      }
    };
    interaction.addEventListener("keydown", (event) => {
      if (event.key.toLowerCase() === "d") {
        event.preventDefault();
        void resolve();
      } else if (event.key.toLowerCase() === "n" || event.key === "Escape") {
        event.preventDefault();
        void dequeue();
      }
    });
    const unsubscribe =
      context.host.data.onChanged?.(() => {
        void refresh();
      }) ?? (() => {});
    void refresh().catch((error) =>
      context.host.data.notify(`Could not load departed Jira issues: ${String(error)}`),
    );
    return () => {
      disposed = true;
      unsubscribe();
      root.replaceChildren();
    };
  },
};

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
    let saveGeneration = 0;
    let saveChain: Promise<boolean> = Promise.resolve(true);
    let authorizationAttempt: string | null = null;
    let connecting = false;
    const save = (next: JiraSettings) => {
      settings = next;
      const generation = ++saveGeneration;
      render();
      const operation = saveChain.then(async () => {
        try {
          const saved = await call<JiraSettings>(context, "jira.settings.update", next);
          if (generation === saveGeneration) settings = saved;
          status = { ...status, last_error: null };
          await context.host.data.changed();
          return true;
        } catch (error) {
          status = { ...status, last_error: String(error) };
          return false;
        } finally {
          if (!disposed) render();
        }
      });
      saveChain = operation;
      return operation;
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
      if (connecting || authorizationAttempt) return;
      connecting = true;
      const attemptId = crypto.randomUUID();
      render();
      try {
        if (!(await save(settings)) || disposed) return;
        authorizationAttempt = attemptId;
        const started = await call<{ authorization_url: string }>(context, "jira.connect.start", {
          attempt_id: attemptId,
        });
        await call(context, "jira.open_browser", { url: started.authorization_url });
        await call(context, "jira.connect.complete", { attempt_id: attemptId });
        const poll = async () => {
          status = await call<JiraStatus>(context, "jira.status");
          if (!status.authorizing && authorizationAttempt === attemptId) {
            authorizationAttempt = null;
            if (status.connected) await context.host.data.changed();
          }
          if (!disposed) render();
          if (status.authorizing && !disposed) setTimeout(() => void poll(), 500);
        };
        await poll();
      } catch (error) {
        try {
          await call(context, "jira.connect.cancel", { attempt_id: attemptId });
        } catch {
          // Preserve the initiating failure while making best effort to release the listener.
        }
        if (authorizationAttempt === attemptId) authorizationAttempt = null;
        status = { ...status, last_error: String(error) };
        if (!disposed) render();
      } finally {
        connecting = false;
        if (!disposed) render();
      }
    };
    const cancelAuthorization = async () => {
      try {
        await call(context, "jira.connect.cancel", { attempt_id: authorizationAttempt });
        authorizationAttempt = null;
        await refresh();
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
          await context.host.data.changed();
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
        connectButton.disabled = !settings.site.trim() || status.authorizing || connecting;
        connectButton.onclick = () => void connect();
        actions.append(connectButton);
        if (status.authorizing) {
          const cancel = document.createElement("button");
          cancel.textContent = "Cancel authorization";
          cancel.onclick = () => void cancelAuthorization();
          actions.append(cancel);
        }
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
            { disabled: status.connected },
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
            input(name, (nextName) => {
              if (!nextName || nextName === name || sources[nextName]) return;
              const next = { ...sources, [nextName]: source };
              delete next[name];
              void save({ ...settings, sources: next });
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
          const jiraStatusInput = input(
            jiraStatus,
            (nextStatus) => {
              const next = { ...map };
              delete next[jiraStatus];
              if (nextStatus) next[nextStatus] = planeaiStatus;
              void save({
                ...settings,
                sources: { ...sources, [name]: { ...source, status_map: next } },
              });
            },
            "",
            { ariaLabel: "Jira status" },
          );
          row.append(jiraStatusInput);
          const select = document.createElement("select");
          select.setAttribute("aria-label", `PlaneAI status for ${jiraStatus}`);
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
          removeMap.setAttribute("aria-label", `Remove status mapping for ${jiraStatus}`);
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
          sources: {
            ...sources,
            [name]: {
              jql: "key = __PLANEAI_CONFIGURE_SOURCE__",
              status_map: {},
              writeback: null,
            },
          },
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
      const attemptId = authorizationAttempt;
      authorizationAttempt = null;
      if (attemptId) void call(context, "jira.connect.cancel", { attempt_id: attemptId });
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
    let assignmentModal: PluginModalControls | null = null;
    let disposed = false;
    const render = (focusRowId: string | null = null) => {
      unregister();
      const header = document.createElement("button");
      header.className = `section-header ${selected === "header" ? "selected" : ""}`;
      header.setAttribute("aria-expanded", String(!collapsed));
      header.setAttribute("aria-controls", "jira-sidebar-issues");
      header.textContent = `${collapsed ? "›" : "⌄"} Jira`;
      const count = document.createElement("span");
      count.className = "count";
      count.textContent = String(items.length);
      header.append(count);
      header.onclick = () => {
        context.host.sidebar.select("header");
        selected = "header";
        collapsed = !collapsed;
        render("header");
      };
      header.onkeydown = context.host.sidebar.handleKeydown;
      const issueRegion = document.createElement("div");
      issueRegion.id = "jira-sidebar-issues";
      issueRegion.setAttribute("role", "group");
      issueRegion.setAttribute("aria-label", "Jira issues");
      section.replaceChildren(header, issueRegion);
      if (focusRowId === "header") header.focus();
      if (!collapsed)
        for (const item of items) {
          const rowId = `issue:${item.key}`;
          const row = document.createElement("button");
          row.className = `issue ${selected === item.key ? "selected" : ""}`;
          row.innerHTML = `<span class="dot ${item.status === "done" ? "done" : item.status !== "todo" ? "active" : ""}"></span><span class="key"></span><span class="title"></span>${item.child_count ? `<span class="count">${item.child_count}</span>` : ""}`;
          row.setAttribute(
            "aria-label",
            `${item.key}: ${item.title}. Status: ${sidebarStatusLabel(item.status)}`,
          );
          row.querySelector(".key")!.textContent = item.key;
          row.querySelector(".title")!.textContent = item.title;
          row.onclick = () => {
            context.host.sidebar.select(rowId);
            selected = item.key;
            render(rowId);
            assignmentModal?.close();
            assignmentModal = openAssignment(context, item.key);
          };
          row.onkeydown = (event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              event.stopPropagation();
              row.click();
              return;
            }
            context.host.sidebar.handleKeydown(event);
          };
          issueRegion.append(row);
          if (focusRowId === rowId) row.focus();
        }
      unregister = context.host.sidebar.register([
        {
          id: "header",
          onSelect: () => {
            collapsed = !collapsed;
            render("header");
          },
          onCollapse: () => {
            collapsed = true;
            render("header");
          },
          onExpand: () => {
            collapsed = false;
            render("header");
          },
          onFocus: (active) => {
            if (!active) {
              if (selected) {
                selected = "";
                render();
              }
              return;
            }
            if (active && selected !== "header") {
              selected = "header";
              render("header");
              header.scrollIntoView({ block: "nearest" });
            }
          },
        },
        ...(!collapsed
          ? items.map((item) => ({
              id: `issue:${item.key}`,
              onSelect: () => {
                selected = item.key;
                render(`issue:${item.key}`);
              },
              onCollapse: () => {
                selected = "header";
                render("header");
              },
              onFocus: (active: boolean) => {
                if (!active) {
                  if (selected) {
                    selected = "";
                    render();
                  }
                  return;
                }
                if (active && selected !== item.key) {
                  selected = item.key;
                  render(`issue:${item.key}`);
                }
              },
            }))
          : []),
      ]);
    };
    let refreshRunning = false;
    let refreshAgain = false;
    const refresh = async () => {
      if (refreshRunning) {
        refreshAgain = true;
        return;
      }
      refreshRunning = true;
      try {
        do {
          refreshAgain = false;
          const value = await call<{ items: SidebarItem[] }>(context, "jira.sidebar.items");
          items = value.items;
          if (!disposed) render();
        } while (refreshAgain && !disposed);
      } finally {
        refreshRunning = false;
      }
    };
    const requestRefresh = () => {
      void refresh().catch(() => {
        if (!disposed) render();
      });
    };
    const unsubscribeDataChanged = context.host.data.onChanged?.(requestRefresh) ?? (() => {});
    const unsubscribeTaskDataChanged =
      context.host.data.onTaskDataChanged?.(requestRefresh) ?? (() => {});
    requestRefresh();
    return () => {
      disposed = true;
      unsubscribeDataChanged();
      unsubscribeTaskDataChanged();
      assignmentModal?.dispose();
      unregister();
      root.replaceChildren();
    };
  },
};
export const jiraStatusEntrypoint: PluginUiEntrypoint = jiraPreferencesEntrypoint;
