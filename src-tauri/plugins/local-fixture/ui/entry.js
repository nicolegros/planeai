const fixtureEntrypoint = {
  mount(root, context) {
    const page = document.createElement("main");
    page.className = "plugin-page";
    page.innerHTML = `
      <style>
        :host { color: var(--color-t1); font-family: var(--font-sans); }
        .plugin-page { height: 100%; box-sizing: border-box; padding: 32px; background: var(--color-main); }
        .card { max-width: 560px; border: 1px solid var(--color-border); border-radius: 10px; padding: 20px; background: var(--color-panel); }
      </style>
      <section class="card"><h1>Local Fixture</h1><p data-status>Loading…</p></section>`;
    root.replaceChildren(page);
    const status = page.querySelector("[data-status]");
    let disposed = false;
    context.host.call("fixture.status").then(
      (value) => {
        if (!disposed) status.textContent = value.runtime_state;
      },
      (error) => {
        if (!disposed) status.textContent = String(error);
      },
    );
    return () => {
      disposed = true;
      root.replaceChildren();
    };
  },
};

export default fixtureEntrypoint;
