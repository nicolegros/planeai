export function nameFromPath(path: string): string {
  return path.replace(/[/\\]$/, "").split(/[/\\]/).pop() || "";
}
