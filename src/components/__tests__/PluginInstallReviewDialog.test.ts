import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import type { StagedPluginInstall } from "../../lib/types";
import PluginInstallReviewDialog from "../PluginInstallReviewDialog.svelte";

const staged: StagedPluginInstall = {
  staging_id: "stage-1",
  repo: "acme/planeai-plugin-acme",
  source_url:
    "https://github.com/acme/planeai-plugin-acme/releases/download/v1.2.0/planeai-plugin-acme-macos-arm64.tar.gz",
  release_tag: "v1.2.0",
  integrity: { kind: "verified", sha256: "3f2a9c1e5b7d" },
  manifest: {
    id: "acme",
    name: "Acme",
    version: "1.2.0",
    capabilities: ["settings", "tools"],
    ui_contributions: [
      {
        id: "settings",
        label: "Acme settings",
        placement: "preferences",
        entrypoint: "ui/settings.js",
        order: null,
        shortcut: null,
      },
    ],
    tools: [
      {
        name: "acme_lookup",
        description: "Look up an Acme record",
        inputSchema: {},
        annotations: {},
      },
    ],
  },
  replaces: { kind: "none" },
  version_mismatch: null,
};

const NATIVE_CODE_WARNING =
  "This is unverified third-party native code. It runs with your user permissions and PlaneAI cannot sandbox it.";

describe("PluginInstallReviewDialog", () => {
  let component: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    if (component) unmount(component);
    document.body.innerHTML = "";
  });

  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');
  const button = (label: string) =>
    Array.from(dialog()?.querySelectorAll<HTMLButtonElement>("button") ?? []).find(
      (candidate) => candidate.textContent?.trim() === label,
    );

  async function open(
    overrides: Partial<StagedPluginInstall>,
    props: { busy?: boolean; onConfirm?: () => void; onCancel?: () => void } = {},
  ) {
    component = mount(PluginInstallReviewDialog, {
      target: document.body,
      props: {
        staged: { ...staged, ...overrides },
        busy: props.busy ?? false,
        onConfirm: props.onConfirm ?? vi.fn(),
        onCancel: props.onCancel ?? vi.fn(),
      },
    });
    await vi.waitFor(() => expect(dialog()).not.toBeNull());
    return dialog()!;
  }

  it("lists the package facts with a Verified badge and the archive hash", async () => {
    const content = await open({});
    const text = content.textContent ?? "";
    expect(text).toContain("Install Acme?");
    expect(text).toContain("github.com/acme/planeai-plugin-acme");
    expect(text).toContain(staged.source_url);
    expect(text).toContain("v1.2.0");
    expect(text).toContain("Verified");
    expect(text).not.toContain("Unverified");
    const labels = Array.from(content.querySelectorAll("dt")).map((row) => row.textContent);
    expect(labels).toEqual([
      "id",
      "version",
      "repository",
      "source",
      "release",
      "archive sha-256",
      "capabilities",
      "ui",
      "tools",
    ]);
    expect(text).toContain("3f2a9c1e5b7d");
    expect(text).toContain("settings, tools");
    expect(text).toContain("Acme settings (preferences)");
    expect(text).toContain("acme_lookup: Look up an Acme record");
    expect(text).toContain(NATIVE_CODE_WARNING);
    expect(text).not.toContain("Data and settings are kept.");
  });

  it("marks an unverified package and falls back to none for empty lists", async () => {
    const content = await open({
      integrity: { kind: "unverified", sha256: "deadbeef" },
      release_tag: null,
      manifest: { ...staged.manifest, capabilities: [], ui_contributions: [], tools: [] },
    });
    const text = content.textContent ?? "";
    expect(text).toContain("Unverified");
    expect(text).toContain("deadbeef");
    expect(text).toContain(NATIVE_CODE_WARNING);
    const rows = Array.from(content.querySelectorAll("dd")).map((row) => row.textContent?.trim());
    expect(rows).toContain("none");
    expect(rows.filter((row) => row === "none")).toHaveLength(4);
  });

  it("explains a local build replacement with its original path", async () => {
    const content = await open({
      replaces: {
        kind: "local",
        installed_version: "0.9.0",
        original_display_path: "/source/acme",
      },
    });
    expect(content.textContent).toContain(
      "Replaces your local build v0.9.0 at /source/acme. Data and settings are kept.",
    );
  });

  it("explains a local build replacement without an original path", async () => {
    const content = await open({
      replaces: { kind: "local", installed_version: "0.9.0", original_display_path: null },
    });
    expect(content.textContent).toContain(
      "Replaces your local build v0.9.0. Data and settings are kept.",
    );
  });

  it("explains a replacement of an earlier Discovery install of another version", async () => {
    const content = await open({ replaces: { kind: "repo", installed_version: "1.0.0" } });
    expect(content.textContent).toContain(
      "Replaces v1.0.0 with v1.2.0. Data and settings are kept.",
    );
  });

  it("explains a reinstall of the version already installed from this repository", async () => {
    const content = await open({ replaces: { kind: "repo", installed_version: "1.2.0" } });
    expect(content.textContent).toContain("Reinstalls v1.2.0. Data and settings are kept.");
    expect(content.textContent).not.toContain("Replaces v");
  });

  it("explains a replacement of the same id installed from another repository", async () => {
    const content = await open({
      replaces: {
        kind: "other_repo",
        repo: "fork/planeai-plugin-acme",
        installed_version: "1.0.0",
      },
    });
    expect(content.textContent).toContain(
      "Replaces v1.0.0 installed from github.com/fork/planeai-plugin-acme. Data and settings are kept.",
    );
    expect(content.textContent).not.toContain(" with v");
  });

  it("warns when the release tag and the manifest version disagree", async () => {
    const content = await open({
      release_tag: "v1.3.0",
      version_mismatch: { release_tag: "v1.3.0", manifest_version: "1.2.0" },
    });
    expect(content.textContent).toContain(
      "The release is tagged v1.3.0 but the package manifest says v1.2.0.",
    );
  });

  it("scrolls the package facts while the buttons stay in a fixed footer", async () => {
    const content = await open({});
    const scroller = content.querySelector(".overflow-y-auto");
    expect(scroller?.contains(content.querySelector("dl"))).toBe(true);
    expect(scroller?.textContent).toContain(NATIVE_CODE_WARNING);
    expect(scroller?.contains(button("Install and enable")!)).toBe(false);
    expect(scroller?.contains(button("Cancel")!)).toBe(false);
  });

  it("cancels on Escape while reviewing but stays open while installing", async () => {
    const escape = () =>
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    const onCancel = vi.fn();
    await open({}, { onCancel });
    escape();
    await vi.waitFor(() => expect(onCancel).toHaveBeenCalledOnce());
    unmount(component!);
    component = undefined;
    document.body.innerHTML = "";

    const onCancelWhileBusy = vi.fn();
    await open({}, { busy: true, onCancel: onCancelWhileBusy });
    escape();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(onCancelWhileBusy).not.toHaveBeenCalled();
    expect(dialog()?.textContent).toContain("Install Acme?");
    expect(button("Installing…")?.disabled).toBe(true);
  });

  it("confirms, cancels, and locks both buttons while installing", async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    await open({}, { onConfirm, onCancel });
    button("Install and enable")?.click();
    expect(onConfirm).toHaveBeenCalledOnce();
    button("Cancel")?.click();
    expect(onCancel).toHaveBeenCalledOnce();
    unmount(component!);
    component = undefined;
    document.body.innerHTML = "";

    await open({}, { busy: true, onCancel });
    expect(button("Installing…")?.disabled).toBe(true);
    expect(button("Cancel")?.disabled).toBe(true);
    expect(button("Install and enable")).toBeUndefined();
  });
});
