import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const mocks = vi.hoisted(() => ({ stop: vi.fn(() => Promise.resolve()), detail: vi.fn() }));

vi.mock("../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/api")>();
  return { ...actual, loops: { ...actual.loops, detail: mocks.detail, stop: mocks.stop } };
});
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import LoopDashboard from "../LoopDashboard.svelte";

let component: ReturnType<typeof mount> | undefined;

async function render() {
  mocks.detail.mockResolvedValue({
    run: {
      id: "loop-1",
      status: "running",
      name: "Loop",
      created_at: "2026-10-03T00:00:00Z",
      updated_at: "2026-10-03T00:00:00Z",
    },
    sessions: [],
    events: [],
    artifacts: [],
    verifier_runs: [],
  });
  const target = document.body.appendChild(document.createElement("div"));
  component = mount(LoopDashboard, {
    target,
    props: {
      loopId: "loop-1",
      projectPath: "/tmp/p",
      onSelectSession: vi.fn(),
      onOpenArtifact: vi.fn(),
    },
  });
  await vi.waitFor(() => expect(mocks.detail).toHaveBeenCalled());
  await new Promise((resolve) => setTimeout(resolve, 0));
}

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe("LoopDashboard shortcuts", () => {
  it("ignores bare keys typed inside a dialog over the dashboard", async () => {
    await render();
    const dialog = document.body.appendChild(document.createElement("div"));
    dialog.setAttribute("role", "dialog");
    const heading = dialog.appendChild(document.createElement("h1"));
    heading.tabIndex = -1;
    heading.focus();

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));

    expect(mocks.stop).not.toHaveBeenCalled();
  });

  it("still stops the loop on x when nothing else owns the keyboard", async () => {
    await render();

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));

    expect(mocks.stop).toHaveBeenCalledWith("loop-1");
  });
});
