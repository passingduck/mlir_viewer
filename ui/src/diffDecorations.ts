import type { FunctionDiff, IrSide } from './api'

export type LineClass = 'added' | 'removed' | 'modified'

export function lineClasses(
  diff: FunctionDiff,
  side: IrSide,
  pageOffsetLine: number,
): Map<number, LineClass> {
  const classes = new Map<number, LineClass>()
  const add = (range: [number, number] | null, lineClass: LineClass) => {
    if (!range) return
    for (let line = range[0]; line <= range[1]; line += 1) {
      const localLine = line - pageOffsetLine + 1
      if (localLine >= 1) classes.set(localLine, lineClass)
    }
  }

  for (const change of diff.changes) {
    if (side === 'before') {
      if (change.class === 'removed') add(change.before_lines, 'removed')
      else if (change.class === 'modified') add(change.before_lines, 'modified')
    } else {
      if (change.class === 'added') add(change.after_lines, 'added')
      else if (change.class === 'modified') add(change.after_lines, 'modified')
    }
  }
  return classes
}
