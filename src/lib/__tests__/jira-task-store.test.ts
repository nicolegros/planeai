import { afterEach, describe, expect, it, vi } from "vitest";

const listTasks = vi.hoisted(() => vi.fn());

vi.mock("../api", () => ({
  jira: { listTasks },
}));

import {
  clearJiraTasks,
  getJiraTasks,
  loadJiraTasks,
  loadJiraTasksIfConnected,
} from "../jira-task-store.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe("Jira task store", () => {
  afterEach(() => {
    clearJiraTasks();
    vi.clearAllMocks();
  });

  it("does not restore tasks from a fetch that started before reconnect-required clear", async () => {
    const response = deferred<{
      tasks: Array<{ key: string }>;
      child_counts: Record<string, number>;
    }>();
    listTasks.mockReturnValueOnce(response.promise);

    const loading = loadJiraTasks();
    clearJiraTasks();
    response.resolve({ tasks: [{ key: "PROJ-1" }], child_counts: {} });
    await loading;

    expect(getJiraTasks()).toEqual([]);
  });

  it("does not reload tasks when a queued sync completion arrives after disconnect", async () => {
    clearJiraTasks();

    await loadJiraTasksIfConnected(false);

    expect(listTasks).not.toHaveBeenCalled();
    expect(getJiraTasks()).toEqual([]);
  });
});
