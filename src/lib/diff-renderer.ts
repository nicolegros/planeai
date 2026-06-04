export interface DiffRenderer {
  mount(container: HTMLElement): void;
  setDiff(original: string, modified: string, language: string): void;
  setTheme(theme: string): void;
  setFont(family: string, size: number): void;
  setMode(mode: "side-by-side" | "unified"): void;
  navigateNext(): void;
  navigatePrevious(): void;
  destroy(): void;
}
