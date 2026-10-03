// Reference provider session UI: raw transcript, prompt input and reattach on remount.
const echoChatEntrypoint = {
  mount(root, context) {
    const page = document.createElement("main");
    page.innerHTML = `
      <style>
        main { display: flex; flex-direction: column; height: 100%; padding: var(--planeai-space-4); gap: var(--planeai-space-3); }
        ol { flex: 1; overflow-y: auto; margin: 0; padding: 0; list-style: none; display: grid; align-content: start; gap: var(--planeai-space-2); font-family: var(--planeai-font-mono); white-space: pre-wrap; }
        li[data-kind="user"] { color: var(--planeai-text-muted); }
        form { display: flex; gap: var(--planeai-space-2); }
        input { flex: 1; }
      </style>
      <ol data-transcript aria-live="polite"></ol>
      <form data-composer><input data-prompt type="text" placeholder="Message" aria-label="Message" /><button type="submit">Send</button></form>`;
    root.replaceChildren(page);

    const transcript = page.querySelector("[data-transcript]");
    const composer = page.querySelector("[data-composer]");
    const prompt = page.querySelector("[data-prompt]");
    let lastSeq = 0;
    let disposed = false;

    const render = ({ seq, payload }) => {
      if (disposed || seq <= lastSeq) return;
      lastSeq = seq;
      const line = document.createElement("li");
      line.dataset.kind = payload.type;
      line.textContent = payload.text;
      transcript.append(line);
      transcript.scrollTop = transcript.scrollHeight;
    };
    // Subscribe before the snapshot so nothing emitted in between is lost; seq drops duplicates.
    const buffered = [];
    let replaying = true;
    const unsubscribe = context.host.session.onEvent((event) =>
      replaying ? buffered.push(event) : render(event),
    );
    void context.host
      .call("fixture.providerSnapshot", { session_id: context.session.id })
      .then((snapshot) => snapshot.events.forEach(render))
      .catch((error) => context.host.data.notify(String(error)))
      .finally(() => {
        replaying = false;
        buffered.splice(0).forEach(render);
      });

    const submit = (event) => {
      event.preventDefault();
      const text = prompt.value.trim();
      if (!text) return;
      prompt.value = "";
      void context.host.session
        .send(text)
        .catch((error) => context.host.data.notify(String(error)));
    };
    composer.addEventListener("submit", submit);
    prompt.focus();
    return () => {
      disposed = true;
      unsubscribe();
      composer.removeEventListener("submit", submit);
      root.replaceChildren();
    };
  },
};

export default echoChatEntrypoint;
