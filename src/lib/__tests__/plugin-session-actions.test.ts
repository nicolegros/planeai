import { describe, expect, it } from "vitest";

import { pluginSessionActionsForProvider } from "../plugin-session-actions";

describe("pluginSessionActionsForProvider", () => {
  const actions = [
    { plugin_id: "review", id: "all", label: "All providers", providers: [] },
    { plugin_id: "review", id: "claude", label: "Claude only", providers: ["claude"] },
    { plugin_id: "review", id: "kiro", label: "Kiro only", providers: ["kiro"] },
  ];

  it("keeps unrestricted actions and actions matching the selected provider", () => {
    expect(pluginSessionActionsForProvider(actions, "claude").map((action) => action.id)).toEqual([
      "all",
      "claude",
    ]);
  });

  it("does not expose provider-scoped actions for sessions without a provider", () => {
    expect(pluginSessionActionsForProvider(actions, null).map((action) => action.id)).toEqual(["all"]);
  });
});
