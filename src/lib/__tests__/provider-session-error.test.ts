import { describe, expect, it } from "vitest";
import { errorMessage } from "../errors";
import {
  frameFailure,
  ProviderSessionError,
  toProviderSessionError,
} from "../provider-session-error";

describe("provider session errors", () => {
  it("types the host's refusals and leaves anything else as is", () => {
    const refused = toProviderSessionError({ code: "handed_off", message: "In a terminal." });
    expect(refused).toBeInstanceOf(ProviderSessionError);
    expect(refused).toMatchObject({ code: "handed_off", message: "In a terminal." });
    for (const other of [
      "plugin chat is not running",
      { code: "teleported", message: "x" },
      null,
    ]) {
      expect(toProviderSessionError(other)).toBe(other);
    }
  });

  it("gives frames the message, with the code when there is one", () => {
    expect(frameFailure(new ProviderSessionError("unavailable", "Claude is missing."))).toEqual({
      error: "Claude is missing.",
      code: "unavailable",
    });
    expect(frameFailure(new Error("failed"))).toEqual({ error: "failed" });
    expect(frameFailure("plugin is not running")).toEqual({ error: "plugin is not running" });
  });

  it("shows an error's message without its type", () => {
    expect(errorMessage(new ProviderSessionError("not_running", "Gone."))).toBe("Gone.");
    expect(errorMessage("plain")).toBe("plain");
  });
});
