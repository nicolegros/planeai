import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import { openUrl } from "@tauri-apps/plugin-opener";
import { showSnackbar } from "../../lib/snackbar.svelte";
import MarkdownView from "../MarkdownView.svelte";

const mounted: Array<ReturnType<typeof mount>> = [];
afterEach(() => {
  for (const component of mounted.splice(0)) unmount(component);
  document.body.replaceChildren();
  vi.mocked(openUrl).mockReset();
  vi.mocked(showSnackbar).mockReset();
});

function render(source: string) {
  const target = document.body.appendChild(document.createElement("div"));
  mounted.push(mount(MarkdownView, { target, props: { source } }));
  return target;
}

function click(element: Element): MouseEvent {
  const event = new MouseEvent("click", { bubbles: true, cancelable: true });
  element.dispatchEvent(event);
  return event;
}

describe("MarkdownView", () => {
  it("opens https links externally instead of navigating", () => {
    vi.mocked(openUrl).mockResolvedValue();
    const target = render("See [docs](https://example.com/docs).");

    const event = click(target.querySelector("a")!);

    expect(event.defaultPrevented).toBe(true);
    expect(openUrl).toHaveBeenCalledWith("https://example.com/docs");
  });

  it("opens mailto links externally", () => {
    vi.mocked(openUrl).mockResolvedValue();
    const target = render("[mail](mailto:dev@example.com)");

    const event = click(target.querySelector("a")!);

    expect(event.defaultPrevented).toBe(true);
    expect(openUrl).toHaveBeenCalledWith("mailto:dev@example.com");
  });

  it("reports failures to open a link", async () => {
    vi.mocked(openUrl).mockRejectedValue(new Error("denied"));
    const target = render("https://example.com");

    click(target.querySelector("a")!);

    await vi.waitFor(() =>
      expect(showSnackbar).toHaveBeenCalledWith("Failed to open URL: Error: denied"),
    );
  });

  it("ignores clicks outside links", () => {
    const target = render("plain text");

    const event = click(target.querySelector("p")!);

    expect(event.defaultPrevented).toBe(false);
    expect(openUrl).not.toHaveBeenCalled();
  });
});
