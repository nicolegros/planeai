import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("../api", () => ({
  updater: { check: mocks.check },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: mocks.listen,
}));

import {
  _resetForTests,
  checkForUpdates,
  getSettingsUpdateState,
  getUpdateState,
  initUpdateListener,
  resetManualUpdateState,
} from "../updater.svelte";

describe("updater state", () => {
  beforeEach(() => {
    _resetForTests();
    mocks.check.mockReset();
    mocks.listen.mockReset();
  });

  afterEach(() => {
    _resetForTests();
  });

  it("keeps manually discovered updates out of the launch toast", async () => {
    mocks.check.mockResolvedValue({ version: "1.81.0", body: "Release notes" });

    await checkForUpdates();

    expect(getSettingsUpdateState().updateAvailable).toEqual({
      version: "1.81.0",
      body: "Release notes",
    });
    expect(getUpdateState().updateAvailable).toBeNull();
  });

  it("shares launch-discovered updates with Settings and the toast", () => {
    let listener:
      | ((event: { payload: { version: string; body: string | null } }) => void)
      | undefined;
    mocks.listen.mockImplementation((_event, callback) => {
      listener = callback;
      return Promise.resolve(() => {});
    });

    initUpdateListener();
    listener?.({ payload: { version: "1.81.0", body: null } });

    expect(getUpdateState().updateAvailable?.version).toBe("1.81.0");
    expect(getSettingsUpdateState().updateAvailable?.version).toBe("1.81.0");
  });

  it("retains up-to-date and full-error feedback until Settings closes", async () => {
    mocks.check.mockResolvedValueOnce(null);
    await checkForUpdates();
    expect(getSettingsUpdateState().upToDate).toBe(true);

    mocks.check.mockRejectedValueOnce(new Error("updater endpoint unavailable"));
    await checkForUpdates();
    expect(getSettingsUpdateState().checkError).toBe("Error: updater endpoint unavailable");

    resetManualUpdateState();
    expect(getSettingsUpdateState().upToDate).toBe(false);
    expect(getSettingsUpdateState().checkError).toBeNull();
  });
});
