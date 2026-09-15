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

  it("shares launch-discovered updates with Settings and the toast", async () => {
    let listener:
      | ((event: { payload: { version: string; body: string | null } }) => void)
      | undefined;
    mocks.listen.mockImplementation((_event, callback) => {
      listener = callback;
      return Promise.resolve(() => {});
    });

    await initUpdateListener();
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

  it("ignores a manual check completing after Settings closes", async () => {
    let resolveCheck: (update: { version: string; body: null }) => void;
    mocks.check.mockReturnValue(
      new Promise((resolve) => {
        resolveCheck = resolve;
      }),
    );

    const checking = checkForUpdates();
    resetManualUpdateState();
    resolveCheck!({ version: "1.81.0", body: null });
    await checking;

    expect(getSettingsUpdateState().checking).toBe(false);
    expect(getSettingsUpdateState().updateAvailable).toBeNull();
  });

  it("handles a failed listener registration and allows retry", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.listen.mockRejectedValueOnce(new Error("Tauri unavailable"));
    mocks.listen.mockResolvedValueOnce(() => {});

    await initUpdateListener();
    await initUpdateListener();

    expect(mocks.listen).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });

  it("waits for an in-flight listener registration", async () => {
    let resolveRegistration: (unlisten: () => void) => void;
    mocks.listen.mockReturnValue(
      new Promise<() => void>((resolve) => {
        resolveRegistration = resolve;
      }),
    );

    const first = initUpdateListener();
    let secondFinished = false;
    const second = initUpdateListener().then(() => {
      secondFinished = true;
    });
    await Promise.resolve();

    expect(mocks.listen).toHaveBeenCalledOnce();
    expect(secondFinished).toBe(false);

    resolveRegistration!(() => {});
    await Promise.all([first, second]);
    expect(secondFinished).toBe(true);
  });
});
