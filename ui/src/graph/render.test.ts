import { describe, expect, it } from 'vitest'
import { hitTest, nodeColor, type Layout } from './render'

describe('render helpers', () => {
  it('colors nodes by change class', () => {
    expect(nodeColor('added')).toBe('#2ea043')
    expect(nodeColor('removed')).toBe('#f85149')
    expect(nodeColor('modified')).toBe('#d2a83c')
    expect(nodeColor(undefined)).toBe('#3a4658')
  })

  it('hit-tests a node by world coordinates', () => {
    const layout: Layout = {
      positions: { op0: { x: 10, y: 10, width: 100, height: 40 } },
      edges: [],
    }
    expect(hitTest(layout, 50, 30)).toBe('op0')
    expect(hitTest(layout, 500, 500)).toBeNull()
  })
})
