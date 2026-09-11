import { describe, expect, it } from "vitest";
import {
  emptyLanguageServerProfileDraft,
  languageServerProfileDraft,
  validateLanguageServerProfile,
} from "../language-server-profile";

describe("language-server profiles", () => {
  it("normalizes extensions and preserves newline-delimited arguments", () => {
    const result = validateLanguageServerProfile(
      {
        id: " local-rust-analyzer ",
        languageId: " rust ",
        extensions: " .rs, RS,  ",
        command: " /opt/bin/rust-analyzer ",
        args: "--stdio\n--config\ncheck.command=clippy",
        enabled: true,
      },
      [],
    );

    expect(result).toEqual({
      ok: true,
      profile: {
        id: "local-rust-analyzer",
        language_id: "rust",
        extensions: ["rs"],
        command: "/opt/bin/rust-analyzer",
        args: ["--stdio", "--config", "check.command=clippy"],
        enabled: true,
      },
    });
  });

  it("rejects incomplete and duplicate profiles", () => {
    expect(validateLanguageServerProfile(emptyLanguageServerProfileDraft(), [])).toEqual({
      ok: false,
      error: "Profile ID is required.",
    });
    expect(validateLanguageServerProfile(
      { id: "rust", languageId: "rust", extensions: "rs", command: "rust-analyzer", args: "", enabled: true },
      ["rust"],
    )).toEqual({ ok: false, error: "A profile named “rust” already exists." });
  });

  it("round-trips a saved profile into an editable draft", () => {
    expect(languageServerProfileDraft({
      id: "pyright",
      language_id: "python",
      extensions: ["py", "pyi"],
      command: "pyright-langserver",
      args: ["--stdio", "--verbose"],
      enabled: null,
    })).toEqual({
      id: "pyright",
      languageId: "python",
      extensions: "py, pyi",
      command: "pyright-langserver",
      args: "--stdio\n--verbose",
      enabled: true,
    });
  });
});
