// Stub matchMedia for jsdom (not implemented natively)
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }),
});

if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => [];
}

// jsdom lacks the Web Animations API that row motion (animate:flip, transitions) runs on; every
// animation finishes at once.
Element.prototype.getAnimations = () => [];
Element.prototype.animate = function () {
  const animation = {
    currentTime: 0,
    playState: "finished",
    onfinish: null as (() => void) | null,
  };
  queueMicrotask(() => animation.onfinish?.());
  return Object.assign(animation, { cancel() {} }) as unknown as Animation;
};
