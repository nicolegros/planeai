import { describe, expect, it, vi } from "vitest";

vi.mock("../settings.svelte", () => ({
  getSettings: () => ({
    providers: { claude: {}, "chat:claude": {} },
    default_provider: "claude",
  }),
}));

import { ProviderChoice } from "../provider-choice.svelte";
import type { RuntimeProvider } from "../plugin-providers";

const chat = {
  key: "chat:claude",
  plugin: { id: "chat" },
  provider: { id: "claude", label: "Claude Chat", entrypoint: "ui/chat.js", supports: [] },
} as unknown as RuntimeProvider;

describe("ProviderChoice", () => {
  it("lists a plugin's provider once, even when the config also names its key", () => {
    const choice = new ProviderChoice(() => [chat]);
    expect(choice.keys).toEqual(["claude", "chat:claude"]);
    expect(choice.label("chat:claude")).toBe("Claude Chat");
  });

  it("turns auto-approve off for providers without it, keeping the user's choice", () => {
    const choice = new ProviderChoice(() => [chat]);
    expect(choice.autoApprove(true)).toBe(true);
    choice.cycle(1);
    expect(choice.key).toBe("chat:claude");
    expect(choice.autoApprove(true)).toBe(false);
    expect(choice.autoApproveBlocked).toBe("Claude Chat does not support auto-approve");
    choice.cycle(1);
    expect(choice.autoApprove(true)).toBe(true);
  });
});
