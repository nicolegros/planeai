import { describe, expect, it } from "vitest";
import { contentFingerprint, type ReviewFileDiff, type TextFileDiff } from "../review-diff";

const textDiff: TextFileDiff = {
  kind: "text",
  original: "before\n",
  modified: "after\n",
  language: "typescript",
  original_size: 7,
  modified_size: 6,
};

describe("review diff data", () => {
  it("keeps text diffs discriminated from binary responses", () => {
    const diffs: ReviewFileDiff[] = [
      textDiff,
      { kind: "binary", language: "", original_size: 12, modified_size: 28 },
    ];

    expect(diffs.filter((diff): diff is TextFileDiff => diff.kind === "text")).toEqual([textDiff]);
  });

  it("changes the snapshot fingerprint when either reviewed document changes", () => {
    const fingerprint = contentFingerprint(textDiff);
    expect(contentFingerprint({ ...textDiff, original: "other\n" })).not.toBe(fingerprint);
    expect(contentFingerprint({ ...textDiff, modified: "other\n" })).not.toBe(fingerprint);
    expect(contentFingerprint({ ...textDiff })).toBe(fingerprint);
  });
});
