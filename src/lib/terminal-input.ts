/** Write user-originated terminal bytes only after dependent UI state is invalidated. */
export function writeUserInput(
  bytes: number[],
  invalidate: () => void,
  write: (bytes: number[]) => void,
): void {
  invalidate();
  write(bytes);
}
