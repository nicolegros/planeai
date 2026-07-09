import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

/**
 * Tests for the font-atlas garbled glyph fix (PLA-227).
 *
 * The Terminal component now:
 *  1. Waits for the configured font to load before calling term.open()
 *  2. Calls clearTextureAtlas() on the WebGL addon every time visibility is restored
 *  3. Calls clearTextureAtlas() after the WebGL addon first loads
 *  4. Calls clearTextureAtlas() on font/theme settings changes
 *
 * The bug: when switching between sessions, the WebGL addon's texture atlas
 * can become stale (browser may evict GPU textures for off-screen canvases).
 * Characters appear as garbled glyphs until a resize forces atlas rebuild.
 * Resizing fixes it because fitAddon.fit() triggers xterm to re-measure cell
 * dimensions, which causes the WebGL renderer to rebuild its texture atlas.
 *
 * These tests validate the extracted fix logic:
 *  - waitForFont resolves when the font is available
 *  - clearTextureAtlas is called on every visibility restore
 *  - clearTextureAtlas is called when WebGL addon first loads
 *  - clearTextureAtlas is called on settings change
 *  - WebGL addon is not loaded before term.open() (opened guard)
 */

// ── Mocked WebGL addon ──────────────────────────────────────────────────

function createMockWebglAddon() {
  return {
    clearTextureAtlas: vi.fn(),
    dispose: vi.fn(),
    onContextLoss: vi.fn(),
  };
}

function createMockTerminal() {
  return {
    rows: 24,
    cols: 80,
    open: vi.fn(),
    loadAddon: vi.fn(),
    options: {} as Record<string, unknown>,
  };
}

function createMockFitAddon() {
  return { fit: vi.fn() };
}

// ── Extracted visibility-restore logic (mirrors Terminal.svelte $effect) ──

interface FontAtlasState {
  term: ReturnType<typeof createMockTerminal> | null;
  opened: boolean;
  webglAddon: ReturnType<typeof createMockWebglAddon> | null;
  lastSentDims: { cols: number; rows: number } | null;
}

/**
 * Mirrors the visibility $effect body for `visible = true`
 */
function onVisibilityRestore(
  state: FontAtlasState,
  createAddon: () => ReturnType<typeof createMockWebglAddon>,
) {
  if (!state.term || !state.opened) return;

  if (!state.webglAddon) {
    const addon = createAddon();
    state.term.loadAddon(addon);
    state.webglAddon = addon;
    state.webglAddon.clearTextureAtlas();
  } else {
    state.webglAddon.clearTextureAtlas();
  }
}

/**
 * Mirrors the visibility $effect body for `visible = false`
 */
function onVisibilityLost(state: FontAtlasState) {
  state.lastSentDims = null;
}

/**
 * Mirrors the settings change $effect
 */
function onSettingsChange(state: FontAtlasState, fitAddon: ReturnType<typeof createMockFitAddon>) {
  if (!state.term) return;
  if (state.webglAddon) state.webglAddon.clearTextureAtlas();
  fitAddon.fit();
}

// ── waitForFont logic ────────────────────────────────────────────────────

async function waitForFont(family: string, size: number): Promise<void> {
  try {
    await document.fonts.load(`${size}px "${family}"`);
  } catch {
    await document.fonts.ready;
  }
}

// ── Tests ────────────────────────────────────────────────────────────────

describe("Terminal font-atlas garbled glyph fix (PLA-227)", () => {
  let state: FontAtlasState;
  let fitAddon: ReturnType<typeof createMockFitAddon>;

  beforeEach(() => {
    state = {
      term: createMockTerminal(),
      opened: true,
      webglAddon: null,
      lastSentDims: { cols: 80, rows: 24 },
    };
    fitAddon = createMockFitAddon();
  });

  describe("clearTextureAtlas on visibility restore", () => {
    it("clears atlas when webgl addon already exists (session switch back)", () => {
      const addon = createMockWebglAddon();
      state.webglAddon = addon;

      // Simulate: switch away then back
      onVisibilityLost(state);
      onVisibilityRestore(state, createMockWebglAddon);

      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(1);
    });

    it("clears atlas on first visibility (addon creation)", () => {
      // No addon yet — first time becoming visible
      const addon = createMockWebglAddon();
      onVisibilityRestore(state, () => addon);

      expect(state.term!.loadAddon).toHaveBeenCalledWith(addon);
      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(1);
    });

    it("clears atlas on every subsequent visibility restore", () => {
      const addon = createMockWebglAddon();
      state.webglAddon = addon;

      // Multiple session switches
      onVisibilityLost(state);
      onVisibilityRestore(state, createMockWebglAddon);
      onVisibilityLost(state);
      onVisibilityRestore(state, createMockWebglAddon);
      onVisibilityLost(state);
      onVisibilityRestore(state, createMockWebglAddon);

      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(3);
    });
  });

  describe("opened guard prevents premature WebGL addon loading", () => {
    it("does NOT load WebGL addon before term.open() completes", () => {
      state.opened = false; // font still loading, term not opened yet

      onVisibilityRestore(state, createMockWebglAddon);

      expect(state.webglAddon).toBeNull();
      expect(state.term!.loadAddon).not.toHaveBeenCalled();
    });

    it("loads WebGL addon after term.open() completes", () => {
      state.opened = false;

      // Visibility effect fires before open — no addon
      onVisibilityRestore(state, createMockWebglAddon);
      expect(state.webglAddon).toBeNull();

      // Font loads, term opens
      state.opened = true;

      // Visibility effect re-fires (Svelte reactivity on `opened`)
      onVisibilityRestore(state, createMockWebglAddon);
      expect(state.webglAddon).not.toBeNull();
    });
  });

  describe("clearTextureAtlas on settings change", () => {
    it("clears atlas when font settings change", () => {
      const addon = createMockWebglAddon();
      state.webglAddon = addon;

      onSettingsChange(state, fitAddon);

      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(1);
      expect(fitAddon.fit).toHaveBeenCalledTimes(1);
    });

    it("does not error when no webgl addon loaded", () => {
      state.webglAddon = null;
      expect(() => onSettingsChange(state, fitAddon)).not.toThrow();
      expect(fitAddon.fit).toHaveBeenCalledTimes(1);
    });
  });

  describe("waitForFont", () => {
    let originalFonts: FontFaceSet;

    beforeEach(() => {
      originalFonts = document.fonts;
    });

    afterEach(() => {
      Object.defineProperty(document, "fonts", { value: originalFonts, configurable: true });
    });

    it("resolves when document.fonts.load succeeds", async () => {
      const mockLoad = vi.fn().mockResolvedValue([]);
      Object.defineProperty(document, "fonts", {
        value: { load: mockLoad, ready: Promise.resolve() },
        configurable: true,
      });

      await expect(waitForFont("Menlo", 14)).resolves.toBeUndefined();
      expect(mockLoad).toHaveBeenCalledWith('14px "Menlo"');
    });

    it("falls back to document.fonts.ready when load throws", async () => {
      const mockLoad = vi.fn().mockRejectedValue(new Error("not supported"));
      Object.defineProperty(document, "fonts", {
        value: { load: mockLoad, ready: Promise.resolve() },
        configurable: true,
      });

      await expect(waitForFont("Unknown Font", 14)).resolves.toBeUndefined();
      expect(mockLoad).toHaveBeenCalledWith('14px "Unknown Font"');
    });
  });

  describe("full session-switch scenario", () => {
    it("atlas is rebuilt when switching sessions back and forth", () => {
      const addon = createMockWebglAddon();

      // Session first becomes visible — addon loaded with atlas clear
      onVisibilityRestore(state, () => addon);
      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(1);

      // Switch to another session (this one goes hidden)
      onVisibilityLost(state);
      expect(state.lastSentDims).toBeNull();

      // Switch back — atlas cleared again
      onVisibilityRestore(state, createMockWebglAddon);
      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(2);
    });

    it("handles rapid switching without errors", () => {
      const addon = createMockWebglAddon();
      state.webglAddon = addon;

      // Rapid toggling
      for (let i = 0; i < 10; i++) {
        onVisibilityLost(state);
        onVisibilityRestore(state, createMockWebglAddon);
      }

      expect(addon.clearTextureAtlas).toHaveBeenCalledTimes(10);
    });
  });
});
