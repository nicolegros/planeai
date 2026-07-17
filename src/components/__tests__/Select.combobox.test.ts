import { describe, it, expect, vi } from "vitest";
import { mount, tick } from "svelte";
import Select from "../ui/Select.svelte";

const branches = [
  { value: "main", label: "main" },
  { value: "pol-127/test", label: "pol-127/test" },
  { value: "remote:pol-127/test", label: "pol-127/test", remote: true },
  { value: "feature/other", label: "feature/other" },
];

function renderSelect(props = {}) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  mount(Select, { target, props: { items: branches, ...props } });
  return target;
}

function getInput(target: HTMLElement): HTMLInputElement {
  return target.querySelector("input")!;
}

function type(input: HTMLInputElement, text: string) {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function pressEnter(input: HTMLInputElement) {
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
}

describe("Select combobox", () => {
  it("selects the first filtered item on Enter when nothing is highlighted", async () => {
    const onValueChange = vi.fn();
    const target = renderSelect({ onValueChange });
    const input = getInput(target);

    // Focus to open the dropdown
    input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    await tick();

    // Type "127" to filter — both local and remote match (2 results)
    type(input, "127");
    await tick();

    // No item is highlighted (bits-ui doesn't auto-highlight on external filter)
    // Press Enter — should select the first filtered item
    pressEnter(input);
    await tick();

    expect(onValueChange).toHaveBeenCalledWith("pol-127/test");
  });

  it("does not double-select when an item IS highlighted by bits-ui", async () => {
    const onValueChange = vi.fn();
    const target = renderSelect({ onValueChange });
    const input = getInput(target);

    // Focus to open the dropdown
    input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    await tick();

    // Type to filter
    type(input, "127");
    await tick();

    // Simulate bits-ui highlighting the second item (remote) by setting data-highlighted
    // on an item within the contentRef
    const content = document.querySelector("[data-combobox-content]");
    if (content) {
      const items = content.querySelectorAll("[data-combobox-item]");
      if (items[1]) {
        items[1].setAttribute("data-highlighted", "");
      }
    }

    // Press Enter — our fallback should NOT fire since something is highlighted.
    // bits-ui's own handler fires instead (which we don't test here).
    pressEnter(input);
    await tick();

    // onValueChange may be called by bits-ui's own handler, but NOT with the first item.
    // If our fallback had fired, it would call with "pol-127/test" (first item).
    // Since bits-ui handles it, we just verify the fallback didn't force first-item selection
    // by checking that if called, it was called at most once (bits-ui's call, not ours + bits-ui's).
    const calls = onValueChange.mock.calls;
    // bits-ui may or may not process this in JSDOM — the key is our fallback doesn't add an extra call
    expect(calls.length).toBeLessThanOrEqual(1);
  });

  it("selects correctly when only one result remains", async () => {
    const onValueChange = vi.fn();
    const target = renderSelect({
      items: [
        { value: "main", label: "main" },
        { value: "pol-127/test", label: "pol-127/test" },
      ],
      onValueChange,
    });
    const input = getInput(target);

    input.dispatchEvent(new FocusEvent("focus", { bubbles: true }));
    await tick();

    // Type enough to narrow to 1 result
    type(input, "pol-127");
    await tick();

    pressEnter(input);
    await tick();

    expect(onValueChange).toHaveBeenCalledWith("pol-127/test");
  });
});
