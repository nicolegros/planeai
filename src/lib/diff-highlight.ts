/**
 * Diff Highlight — manages progressive highlighting with caching, queuing, and prefetching.
 * Modeled after Hunk's approach: render plain first, highlight async with timer yields.
 */
import {
  getSharedHighlighter,
  getHighlighterOptions,
  preloadHighlighter,
  isHighlighterLoaded,
} from "@pierre/diffs";

const MAX_CACHE_ENTRIES = 30;

/** Rendered HTML cache keyed by file+theme+style */
const htmlCache = new Map<string, string>();

function enforceCacheLimit() {
  while (htmlCache.size > MAX_CACHE_ENTRIES) {
    const oldest = htmlCache.keys().next().value;
    if (oldest !== undefined) htmlCache.delete(oldest);
  }
}

export function buildCacheKey(filePath: string, themeType: string, diffStyle: string): string {
  return `${filePath}:${themeType}:${diffStyle}`;
}

export function getCachedHTML(key: string): string | undefined {
  return htmlCache.get(key);
}

export function setCachedHTML(key: string, html: string): void {
  htmlCache.set(key, html);
  enforceCacheLimit();
}

export function invalidateCache(filePath?: string): void {
  if (!filePath) {
    htmlCache.clear();
    return;
  }
  for (const key of htmlCache.keys()) {
    if (key.startsWith(filePath + ":")) htmlCache.delete(key);
  }
}

/** Queue highlight work with setTimeout(0) yields to prevent UI freezing */
let queuedWork = Promise.resolve();

export function queueHighlightWork<T>(run: () => T): Promise<T> {
  const queued = queuedWork.then(
    () => new Promise<T>((resolve, reject) => {
      setTimeout(() => {
        try { resolve(run()); }
        catch (e) { reject(e); }
      }, 0);
    }),
  );
  queuedWork = queued.then(() => undefined, () => undefined);
  return queued;
}

/** Preload the shiki highlighter so first render can paint immediately */
export async function warmHighlighter(theme: { dark: string; light: string }): Promise<void> {
  if (isHighlighterLoaded()) return;
  await preloadHighlighter(
    getHighlighterOptions("text", { theme }),
  );
}

/** Prefetch highlighting for a file by eagerly loading its language grammar */
export async function prefetchLanguage(filePath: string, theme: { dark: string; light: string }): Promise<void> {
  const ext = filePath.split(".").pop() ?? "";
  const lang = extToLang(ext);
  if (lang === "text") return;
  try {
    await getSharedHighlighter(
      getHighlighterOptions(lang, { theme }),
    );
  } catch {
    // Non-critical — language just won't be pre-loaded
  }
}

function extToLang(ext: string): string {
  const map: Record<string, string> = {
    ts: "typescript", tsx: "tsx", js: "javascript", jsx: "jsx",
    rs: "rust", py: "python", rb: "ruby", go: "go",
    java: "java", kt: "kotlin", swift: "swift", c: "c", cpp: "cpp",
    cs: "csharp", css: "css", scss: "scss", html: "html",
    json: "json", yaml: "yaml", yml: "yaml", toml: "toml",
    md: "markdown", sh: "bash", bash: "bash", zsh: "bash",
    sql: "sql", svelte: "svelte", vue: "vue",
  };
  return map[ext] ?? "text";
}
