import '@testing-library/jest-dom/vitest'

const rect = { x: 0, y: 0, top: 0, left: 0, bottom: 600, right: 800, width: 800, height: 600, toJSON: () => ({}) }

if (typeof ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
}

if (typeof Element.prototype.scrollIntoView === 'undefined') {
  Element.prototype.scrollIntoView = () => {}
}

if (typeof Element.prototype.getBoundingClientRect === 'undefined') {
  Element.prototype.getBoundingClientRect = () => rect
}
