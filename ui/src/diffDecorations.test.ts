import { describe, expect, it } from 'vitest'
import type { FunctionDiff } from './api'
import { lineClasses } from './diffDecorations'

const diff: FunctionDiff = {
  func: 'f',
  changes: [
    {
      class: 'removed',
      before: 1,
      after: null,
      before_lines: [3, 3],
      after_lines: null,
      detail: [],
    },
    {
      class: 'added',
      before: null,
      after: 2,
      before_lines: null,
      after_lines: [4, 4],
      detail: [],
    },
    {
      class: 'modified',
      before: 5,
      after: 5,
      before_lines: [6, 6],
      after_lines: [6, 6],
      detail: ['type'],
    },
  ],
}

describe('lineClasses', () => {
  it('maps removed and modified to the before pane', () => {
    const classes = lineClasses(diff, 'before', 1)
    expect(classes.get(3)).toBe('removed')
    expect(classes.get(6)).toBe('modified')
    expect(classes.has(4)).toBe(false)
  })

  it('maps added and modified to the after pane', () => {
    const classes = lineClasses(diff, 'after', 1)
    expect(classes.get(4)).toBe('added')
    expect(classes.get(6)).toBe('modified')
    expect(classes.has(3)).toBe(false)
  })

  it('shifts by page offset line', () => {
    const classes = lineClasses(diff, 'before', 3)
    expect(classes.get(1)).toBe('removed')
  })
})
