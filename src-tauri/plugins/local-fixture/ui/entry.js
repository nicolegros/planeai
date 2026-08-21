const fixtureEntrypoint = {
  mount(root, context) {
    const page = document.createElement("main");
    page.className = "plugin-page";
    page.innerHTML = `
      <style>
        :host { color: var(--color-t1); font-family: var(--font-sans); }
        .plugin-page { height: 100%; box-sizing: border-box; padding: 32px; background: var(--color-main); }
        .card { max-width: 560px; border: 1px solid var(--color-border); border-radius: 10px; padding: 20px; background: var(--color-panel); }
        label { display: grid; gap: 6px; margin-top: 16px; }
        input, button { font: inherit; }
        input { border: 1px solid var(--color-border); border-radius: 6px; padding: 8px; background: var(--color-input); color: inherit; }
        button { margin-top: 12px; border: 1px solid var(--color-border); border-radius: 6px; padding: 8px 12px; background: var(--color-accent); color: var(--color-on-accent); }
      </style>
      <section class="card">
        <h1>Local Fixture</h1>
        <p data-status>Loading…</p>
        <label>Greeting <input data-greeting type="text" /></label>
        <button data-save type="button">Save greeting</button>
      </section>`;
    root.replaceChildren(page);

    const status = page.querySelector("[data-status]");
    const greeting = page.querySelector("[data-greeting]");
    const save = page.querySelector("[data-save]");
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
        greeting.value = typeof settings.greeting === "string" ? settings.greeting : "Hello from the fixture";
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

    save.addEventListener("click", saveGreeting);
    void load();
    return () => {
      disposed = true;
      save.removeEventListener("click", saveGreeting);
      root.replaceChildren();
    };
  },
};

export default fixtureEntrypoint;
