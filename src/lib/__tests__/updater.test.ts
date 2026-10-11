import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  install: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("../api", () => ({
  updater: { check: mocks.check, install: mocks.install },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: mocks.listen,
}));

import {
  _resetForTests as resetToasts,
  getToasts,
  requestDismissToast,
  runToastAction,
} from "../toasts.svelte";
import {
  _resetForTests,
  checkForUpdates,
  enableUpdateToast,
  getSettingsUpdateState,
  getUpdateState,
  initUpdateListener,
  resetManualUpdateState,
  setLaunchUpdateAvailable,
} from "../updater.svelte";

describe("updater state", () => {
  beforeEach(() => {
    _resetForTests();
    resetToasts();
    mocks.install.mockReset();
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

describe("update toast", () => {
  const update = { version: "1.81.0", body: null };

  beforeEach(() => {
    _resetForTests();
    resetToasts();
    mocks.install.mockReset();
  });

  afterEach(() => {
    _resetForTests();
    resetToasts();
  });

  it("is only shown in a window that enables it", () => {
    setLaunchUpdateAvailable(update);
    expect(getToasts()).toHaveLength(0);

    enableUpdateToast();
    expect(getToasts()).toEqual([
      expect.objectContaining({
        kind: "info",
        message: "Update available: v1.81.0",
        hiddenDuringOnboarding: true,
        actions: [expect.objectContaining({ key: "i", label: "Install & Restart" })],
      }),
    ]);
  });

  it("installs with i and stays up, undismissable, while installing", async () => {
    mocks.install.mockReturnValue(new Promise(() => {}));
    enableUpdateToast();
    setLaunchUpdateAvailable(update);

    runToastAction(getToasts()[0].id, "i");

    expect(mocks.install).toHaveBeenCalledOnce();
    expect(getUpdateState().installing).toBe(true);
    expect(getToasts()).toEqual([
      expect.objectContaining({ message: "Installing v1.81.0…", actions: [], dismiss: "disabled" }),
    ]);
    expect(requestDismissToast(getToasts()[0].id)).toBe(false);
  });

  it("offers the install again when it fails", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.install.mockRejectedValue(new Error("network"));
    enableUpdateToast();
    setLaunchUpdateAvailable(update);

    runToastAction(getToasts()[0].id, "i");

    await vi.waitFor(() => expect(getUpdateState().installing).toBe(false));
    expect(getToasts()).toEqual([
      expect.objectContaining({ message: "Update available: v1.81.0" }),
    ]);
    error.mockRestore();
  });

  it("stays dismissed for the rest of the run", () => {
    enableUpdateToast();
    setLaunchUpdateAvailable(update);

    expect(requestDismissToast(getToasts()[0].id)).toBe(true);
    expect(getUpdateState().dismissed).toBe(true);

    setLaunchUpdateAvailable({ version: "1.82.0", body: null });
    expect(getToasts()).toHaveLength(0);
  });
});
