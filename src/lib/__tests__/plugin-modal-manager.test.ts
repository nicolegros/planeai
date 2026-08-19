import { afterEach, describe, expect, it } from "vitest";
import { openPluginModal } from "../plugin-modal-manager";

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
