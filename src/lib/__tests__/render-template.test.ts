import { describe, expect, it } from "vitest";
import { renderTemplate } from "../render-template";

// The same literal cases as planeai-core's template tests: the backend renders what this previews.
describe("renderTemplate", () => {
  it("applies the lower, upper and slug transforms", () => {
    const context = { key: "PLA-12", title: "Fix Login Redirect!" };
    expect(renderTemplate("{key:lower}/{title:slug}", context)).toBe("pla-12/fix-login-redirect");
    expect(renderTemplate("{key:upper}: {title}", context)).toBe("PLA-12: Fix Login Redirect!");
    expect(renderTemplate("{title:reverse}", context)).toBe("Fix Login Redirect!");
  });

  it("slugs to ASCII word characters only", () => {
    const slug = (title: string) => renderTemplate("{title:slug}", { title });
    expect(slug("  Hello, World  ")).toBe("hello-world");
    expect(slug("snake_case Name")).toBe("snake_case-name");
    expect(slug("Café déjà vu")).toBe("caf-d-j-vu");
    expect(slug("--a--")).toBe("a");
  });

  it("leaves unknown and non-ASCII placeholders as the frontend always has", () => {
    expect(renderTemplate("{nope}-x", {})).toBe("-x");
    expect(renderTemplate("{tïtle}", { tïtle: "x" })).toBe("{tïtle}");
  });

  it("joins blockers and falls back to the key for an empty parent", () => {
    const task = { key: "PLA-3", parent_key: null, blocked_by: ["PLA-1", "PLA-2"], tags: ["a", "b"], priority: 2 };
    expect(renderTemplate("{parent_key}|{blocked_by}|{tags}|{priority}", task)).toBe(
      "PLA-3|PLA-1, PLA-2|a,b|2",
    );
  });
});
