export type SearchableComboboxItem = {
  value: string;
  label: string;
};

type SearchableComboboxOptions = {
  ariaLabel: string;
  items: SearchableComboboxItem[];
  value: string;
  disabled?: boolean;
  placeholder: string;
  emptyText: string;
  onValueChange(value: string): void;
};

const styles = `.jira-combobox { position:relative; } .jira-combobox-input { width:100%; box-sizing:border-box; border:1px solid var(--color-border); border-radius:4px; background:var(--color-panel); color:var(--color-t1); padding:8px 12px; font:inherit; font-size:14px; } .jira-combobox-input::placeholder { color:var(--color-t3); } .jira-combobox-input:focus { outline:none; box-shadow:0 0 0 1px var(--color-accent); } .jira-combobox-list { position:static; margin-top:4px; max-height:192px; overflow-y:auto; border:1px solid var(--color-border); border-radius:4px; background:var(--color-panel); box-shadow:0 10px 15px -3px var(--color-border); } .jira-combobox-option { display:flex; width:100%; border:0; border-radius:0; background:transparent; color:var(--color-t2); padding:8px 12px; text-align:left; font:inherit; font-size:14px; } .jira-combobox-option:hover,.jira-combobox-option[data-highlighted="true"] { background:var(--color-panel-hi); } .jira-combobox-empty { display:block; padding:8px 12px; color:var(--color-t3); font-size:14px; }`;

/**
 * The app Select component is styled by document-level Tailwind utilities,
 * which cannot cross a plugin ShadowRoot. This mirrors its search and keyboard
 * behavior using the same design tokens inside the plugin boundary.
 */
export function createSearchableCombobox(options: SearchableComboboxOptions): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "jira-combobox";
  const style = document.createElement("style");
  style.textContent = styles;
  const input = document.createElement("input");
  input.className = "jira-combobox-input";
  input.type = "text";
  input.setAttribute("data-field-control", "");
  input.setAttribute("data-jira-project-combobox", "");
  input.setAttribute("aria-label", options.ariaLabel);
  input.setAttribute("role", "combobox");
  input.setAttribute("aria-autocomplete", "list");
  input.autocomplete = "off";
  input.autocorrect = false;
  input.autocapitalize = "off";
  input.spellcheck = false;
  input.disabled = options.disabled ?? false;
  const list = document.createElement("div");
  list.className = "jira-combobox-list";
  list.dataset.layout = "flow";
  list.setAttribute("role", "listbox");
  list.hidden = true;
  const listId = `jira-combobox-${crypto.randomUUID()}`;
  list.id = listId;
  input.setAttribute("aria-controls", listId);

  let open = false;
  let query = "";
  let highlighted = 0;
  const selected = () => options.items.find((item) => item.value === options.value);
  const filtered = () => {
    const normalized = query.trim().toLowerCase();
    return normalized
      ? options.items.filter((item) => item.label.toLowerCase().includes(normalized))
      : options.items;
  };
  const setClosedValue = () => {
    input.value = selected()?.label ?? "";
  };
  const renderOptions = () => {
    const matches = filtered();
    highlighted = Math.min(highlighted, Math.max(matches.length - 1, 0));
    list.replaceChildren();
    if (matches.length === 0) {
      input.removeAttribute("aria-activedescendant");
      const empty = document.createElement("span");
      empty.className = "jira-combobox-empty";
      empty.textContent = options.emptyText;
      list.append(empty);
      return;
    }
    let activeOptionId = "";
    matches.forEach((item, index) => {
      const option = document.createElement("button");
      option.type = "button";
      option.tabIndex = -1;
      option.id = `${listId}-option-${index}`;
      option.className = "jira-combobox-option";
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(item.value === options.value));
      option.dataset.highlighted = String(index === highlighted);
      if (index === highlighted) activeOptionId = option.id;
      option.textContent = item.label;
      option.addEventListener("mousedown", (event) => event.preventDefault());
      option.addEventListener("click", () => choose(item.value));
      list.append(option);
    });
    input.setAttribute("aria-activedescendant", activeOptionId);
  };
  const setOpen = (nextOpen: boolean) => {
    open = nextOpen && !input.disabled;
    list.hidden = !open;
    input.setAttribute("aria-expanded", String(open));
    if (open) renderOptions();
    else input.removeAttribute("aria-activedescendant");
  };
  const choose = (value: string) => {
    options.onValueChange(value);
    query = "";
    setOpen(false);
    input.value = options.items.find((item) => item.value === value)?.label ?? "";
  };

  setClosedValue();
  input.setAttribute("aria-expanded", "false");
  input.placeholder = options.placeholder;
  input.addEventListener("focus", () => {
    if ("preserveSelectedValueOnFocus" in input.dataset) {
      delete input.dataset.preserveSelectedValueOnFocus;
      return;
    }
    query = "";
    highlighted = 0;
    input.value = "";
    setOpen(true);
  });
  input.addEventListener("input", () => {
    query = input.value;
    highlighted = 0;
    setOpen(true);
  });
  input.addEventListener("blur", () => {
    queueMicrotask(() => {
      if (wrap.contains(document.activeElement)) return;
      setOpen(false);
      setClosedValue();
    });
  });
  input.addEventListener("keydown", (event) => {
    const matches = filtered();
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      query = "";
      setOpen(false);
      setClosedValue();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      highlighted =
        event.key === "ArrowDown"
          ? Math.min(highlighted + 1, Math.max(matches.length - 1, 0))
          : Math.max(highlighted - 1, 0);
      setOpen(true);
      list.querySelector<HTMLElement>("[data-highlighted='true']")?.scrollIntoView({
        block: "nearest",
      });
      return;
    }
    if (event.key === "Enter" && open && matches[highlighted]) {
      event.preventDefault();
      event.stopPropagation();
      choose(matches[highlighted].value);
    }
  });
  wrap.append(style, input, list);
  return wrap;
}
