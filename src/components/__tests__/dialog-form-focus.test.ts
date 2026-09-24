/**
 * A modal form driven by the form-keyboard controller owns its own initial
 * focus. bits-ui's dialog focus scope otherwise grabs focus on the next
 * animation frame and lands on the first tabbable field, which:
 *   - drops the form out of NORMAL mode so mnemonic keys stop working, and
 *   - for SessionForm, auto-opens the Project combobox because its input opens
 *     the dropdown on focus.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { mount, flushSync, tick } from "svelte";
import { stubLayoutAsVisible, flushFrames } from "./dom-visibility";

vi.mock("../../lib/api", () => ({
  sessions: { launch: vi.fn(() => new Promise(() => {})) },
  projects: { listBranches: vi.fn(() => Promise.resolve(["main"])) },
  tasks: { list: vi.fn(() => Promise.resolve([])), listAll: vi.fn(() => Promise.resolve([])) },
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    providers: { claude: { command: "claude", yolo_flag: null } },
    default_provider: "claude",
    task_management: {},
  }),
}));

import DialogFormKeyboardHarness from "./DialogFormKeyboardHarness.svelte";
import SessionFormDialogHarness from "./SessionFormDialogHarness.svelte";
import NewItemToSessionHarness from "./NewItemToSessionHarness.svelte";
import TaskFormDialogHarness from "./TaskFormDialogHarness.svelte";
import { createDialogHandoff } from "./dialog-handoff-state.svelte";

stubLayoutAsVisible();

afterEach(() => {
  document.body.innerHTML = "";
});

const sessionFormProps = () => ({
  projects: [{ id: "p1", name: "Project", path: "/tmp/proj", hidden: false }],
  sessions: [],
  onCreated: vi.fn(),
  onCancel: vi.fn(),
  onCreateTask: vi.fn(),
  currentProjectId: "p1",
});

const wrapperEl = () => document.querySelector<HTMLElement>("[data-form-keyboard]")!;

describe("Dialog initial focus", () => {
  it("hands initial focus to a form-keyboard wrapper instead of the first field", async () => {
    mount(DialogFormKeyboardHarness, { target: document.body, props: {} });
    flushSync();
    await flushFrames();

    expect(document.activeElement).toBe(
      document.querySelector<HTMLElement>("[data-testid='fk-wrapper']"),
    );
  });

  it("still honours preventOpenAutoFocus", async () => {
    mount(DialogFormKeyboardHarness, {
      target: document.body,
      props: { preventOpenAutoFocus: true },
    });
    flushSync();
    await flushFrames();

    expect(document.activeElement).toBe(document.body);
  });
});

describe("New Session dialog initial focus", () => {
  function mountSessionDialog() {
    const props = sessionFormProps();
    mount(SessionFormDialogHarness, { target: document.body, props });
    flushSync();
    return props;
  }

  it("opens in NORMAL mode with the form wrapper focused", async () => {
    mountSessionDialog();
    await flushFrames();

    expect(document.activeElement).toBe(wrapperEl());
    expect(document.body.textContent).toContain("NORMAL");
  });

  it("routes mnemonic keys once the dialog has settled", async () => {
    mountSessionDialog();
    await flushFrames();

    const nameInput = document.querySelector<HTMLInputElement>("[data-field='name'] input")!;
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "s", bubbles: true, cancelable: true }),
    );
    await tick();
    flushSync();

    expect(document.activeElement).toBe(nameInput);
  });

  it("returns to NORMAL mode on Escape from a focused field instead of dismissing", async () => {
    const { onCancel } = mountSessionDialog();
    await flushFrames();

    const nameInput = document.querySelector<HTMLInputElement>("[data-field='name'] input")!;
    nameInput.focus();
    await tick();
    flushSync();
    expect(document.body.textContent).toContain("INSERT");

    nameInput.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    await tick();
    flushSync();

    expect(onCancel).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(wrapperEl());
  });

  it("keeps focus when handed off from the New Item dialog (Cmd+N then S)", async () => {
    const terminal = document.createElement("textarea");
    document.body.append(terminal);
    terminal.focus();

    const handoff = createDialogHandoff();
    mount(NewItemToSessionHarness, {
      target: document.body,
      props: { handoff, form: sessionFormProps() },
    });
    flushSync();
    await flushFrames();
    expect(document.activeElement).toBe(terminal);

    handoff.showNewItem = false;
    handoff.showSession = true;
    flushSync();
    await flushFrames();

    expect(document.activeElement).toBe(wrapperEl());
  });
});

describe("New Task dialog initial focus", () => {
  // TaskForm deliberately claims its own first input, so it must stay in INSERT
  // mode on the title field even though the dialog now seeds the wrapper first.
  it("still lands on the title field", async () => {
    mount(TaskFormDialogHarness, {
      target: document.body,
      props: {
        mode: "create",
        projects: [{ id: "p1", name: "Project", path: "/tmp/proj", hidden: false }],
        onSubmitted: vi.fn(),
        onCancel: vi.fn(),
      },
    });
    flushSync();
    await flushFrames();

    const firstInput = wrapperEl().querySelector<HTMLInputElement>("input")!;
    expect(document.activeElement).toBe(firstInput);
    expect(document.body.textContent).toContain("INSERT");
  });
});
