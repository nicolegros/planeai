import { afterEach, describe, expect, it } from "vitest";
import { openPluginModal, openProjectForm } from "../plugin-modal-manager";

describe("plugin modal manager", () => {
  afterEach(() => {
    document.querySelectorAll("[data-plugin-modal]").forEach((element) => element.remove());
  });

  it("closes only the top modal, restores focus, and blocks dismissal while submitting", () => {
    const origin = document.createElement("button");
    document.body.append(origin);
    origin.focus();

    const first = openPluginModal({
      title: "First",
      mount: (root) => root.append(document.createElement("button")),
    });
    const second = openPluginModal({
      title: "Second",
      mount: (root) => root.append(document.createElement("button")),
    });

    first.close();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(2);

    second.setSubmitting(true);
    second.close();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(2);

    second.setSubmitting(false);
    second.close();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(1);

    first.close();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(0);
    expect(document.activeElement).toBe(origin);
    origin.remove();
  });

  it("settles ProjectForm as cancelled on host dismissal or owner disposal", async () => {
    for (const dismiss of ["escape", "backdrop"] as const) {
      const projectForm = openProjectForm();
      const layer = Array.from(document.querySelectorAll<HTMLElement>("[data-plugin-modal]")).at(
        -1,
      )!;
      const dialog = layer.querySelector<HTMLElement>("[role='dialog']")!;
      if (dismiss === "escape") {
        dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      } else {
        layer.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      }
      await expect(projectForm).resolves.toBeNull();
    }

    const assignment = openPluginModal({
      title: "Assignment",
      mount: (root) => root.append(document.createElement("button")),
    });
    const projectForm = openProjectForm();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(2);

    assignment.dispose();
    await expect(projectForm).resolves.toBeNull();
    expect(document.querySelectorAll("[data-plugin-modal]")).toHaveLength(0);
  });

  it("wraps Tab focus between controls in a plugin ShadowRoot", () => {
    let root!: ShadowRoot;
    const controls = openPluginModal({
      title: "Keyboard",
      mount: (nextRoot) => {
        root = nextRoot;
        for (const label of ["First", "Last"]) {
          const button = document.createElement("button");
          button.textContent = label;
          Object.defineProperty(button, "offsetParent", { value: document.body });
          root.append(button);
        }
      },
    });
    const [first, last] = root.querySelectorAll<HTMLButtonElement>("button");

    last.focus();
    last.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, composed: true }));
    expect(root.activeElement).toBe(first);

    first.focus();
    first.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        composed: true,
      }),
    );
    expect(root.activeElement).toBe(last);

    controls.close();
  });

  it("uses a larger content-responsive shell when requested", () => {
    const controls = openPluginModal({
      title: "Responsive",
      contentResponsive: true,
      mount: (root) => root.append(document.createElement("button")),
    });
    const dialog = document.querySelector<HTMLElement>("[data-plugin-modal] [role='dialog']")!;

    expect(dialog.className).toContain("w-[min(90vw,42rem)]");
    expect(dialog.className).toContain("max-h-[90vh]");

    controls.close();
  });

  it("uses the established application dialog shell styling", () => {
    const controls = openPluginModal({
      title: "Styled",
      mount: (root) => root.append(document.createElement("button")),
    });
    const layer = document.querySelector<HTMLElement>("[data-plugin-modal]")!;
    const dialog = layer.querySelector<HTMLElement>("[role='dialog']")!;

    expect(layer.className).not.toContain("bg-black/50");
    expect(layer.className).not.toContain("p-4");
    expect(dialog.className).toContain("w-[452px]");
    expect(dialog.className).toContain("max-h-[85vh]");
    expect(dialog.className).toContain("border-border-s");

    controls.close();
  });
});
