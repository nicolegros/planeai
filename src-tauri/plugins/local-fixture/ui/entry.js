const fixtureEntrypoint = {
  mount(root, context) {
    const page = document.createElement("main");
    page.className = "plugin-page";
    page.innerHTML = `
      <style>
        .plugin-page { height: 100%; padding: var(--planeai-space-6); background: var(--planeai-main); }
        .card { max-width: 560px; border: 1px solid var(--planeai-border); border-radius: var(--planeai-radius); padding: var(--planeai-space-5); background: var(--planeai-surface); }
        .appearance { margin-top: var(--planeai-space-2); color: var(--planeai-text-muted); }
        label { display: grid; gap: 6px; margin-top: var(--planeai-space-4); }
        button { margin-top: var(--planeai-space-3); border-color: var(--planeai-accent); background: var(--planeai-accent); color: var(--planeai-on-accent); }
        :root[data-theme="dark"] .card { border-color: var(--planeai-border-strong); }
      </style>
      <section class="card">
        <h1>Local Fixture</h1>
        <p data-status role="status" aria-live="polite">Loading…</p>
        <p class="appearance" data-appearance></p>
        <label>Greeting <input data-greeting type="text" /></label>
        <button data-save type="button">Save greeting</button>
      </section>`;
    root.replaceChildren(page);

    const status = page.querySelector("[data-status]");
    const greeting = page.querySelector("[data-greeting]");
    const save = page.querySelector("[data-save]");
    const appearance = page.querySelector("[data-appearance]");
    let settings = {};
    let disposed = false;

    const setStatus = (message) => {
      if (!disposed) status.textContent = message;
    };
    const load = async () => {
      try {
        const [savedSettings, runtime] = await Promise.all([
          context.host.settings.get(),
          context.host.call("fixture.status"),
        ]);
        if (disposed) return;
        settings = savedSettings;
        greeting.value =
          typeof settings.greeting === "string" ? settings.greeting : "Hello from the fixture";
        setStatus(`${runtime.runtime_state} · public settings loaded`);
      } catch (error) {
        setStatus(String(error));
      }
    };
    const saveGreeting = async () => {
      try {
        settings = await context.host.settings.replace({ ...settings, greeting: greeting.value });
        await context.host.data.changed();
        setStatus("running · greeting saved");
      } catch (error) {
        setStatus(String(error));
      }
    };

    const showAppearance = ({ mode, preference }) => {
      appearance.textContent = `Appearance: ${mode} (preference: ${preference})`;
    };

    save.addEventListener("click", saveGreeting);
    showAppearance(context.host.theme.get());
    const stopWatchingTheme = context.host.theme.onChange(showAppearance);
    void load();
    return () => {
      disposed = true;
      stopWatchingTheme();
      save.removeEventListener("click", saveGreeting);
      root.replaceChildren();
    };
  },
};

export default fixtureEntrypoint;
