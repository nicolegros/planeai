import { describe, it, expect } from "vitest";
import { ADJECTIVES, NOUNS, randomTaskName } from "../random-task-name";

describe("randomTaskName", () => {
  it("joins an adjective and a noun with a dash", () => {
    expect(randomTaskName(() => 0)).toBe(`${ADJECTIVES[0]}-${NOUNS[0]}`);
  });

  it("picks the last words when the rng approaches 1", () => {
    expect(randomTaskName(() => 0.999999)).toBe(`${ADJECTIVES.at(-1)}-${NOUNS.at(-1)}`);
  });

  it("produces lowercase kebab-case names", () => {
    for (let i = 0; i < 200; i++) {
      expect(randomTaskName()).toMatch(/^[a-z]+-[a-z]+$/);
    }
  });

  it("has word lists without duplicates", () => {
    expect(new Set(ADJECTIVES).size).toBe(ADJECTIVES.length);
    expect(new Set(NOUNS).size).toBe(NOUNS.length);
  });
});
