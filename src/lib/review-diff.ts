export type DiffSide = "original" | "modified";

export interface ReviewSelection {
  side: DiffSide;
  startLine: number;
  endLine: number;
}

export interface TextFileDiff {
  kind: "text";
  original: string;
  modified: string;
  language: string;
  original_size: number;
  modified_size: number;
}

export interface BinaryFileDiff {
  kind: "binary";
  language: string;
  original_size: number;
  modified_size: number;
}

export type ReviewFileDiff = TextFileDiff | BinaryFileDiff;

export function contentFingerprint(diff: TextFileDiff): string {
  return `${diff.original.length}:${diff.modified.length}:${hash(diff.original)}:${hash(diff.modified)}`;
}

function hash(value: string): number {
  let result = 2166136261;
  for (let index = 0; index < value.length; index++) {
    result ^= value.charCodeAt(index);
    result = Math.imul(result, 16777619);
  }
  return result >>> 0;
}
