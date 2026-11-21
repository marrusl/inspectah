import "@testing-library/jest-dom";

// Mock window.matchMedia for jsdom (used by ContainerfilePanel auto-collapse)
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

// jsdom does not implement scrollIntoView, which every reveal calls (often
// from an animation frame, where a missing method surfaces as an unhandled
// error after the test that scheduled it). Tests that assert on scrolling
// spy on this baseline with vi.spyOn and restore it afterwards.
if (typeof Element.prototype.scrollIntoView !== "function") {
  Element.prototype.scrollIntoView = function scrollIntoView() {};
}
