import { fireEvent, render, screen } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import type { IrPage, SelectableOp } from '../api'
import { IrViewer } from './IrViewer'

const before: IrPage = {
  pass_id: 2,
  side: 'before',
  text: 'module { %0 = arith.constant 0 : i32 }',
  offset: 0,
  next_offset: null,
  total_bytes: 38,
}

test('creates a read-only editor and explains a missing side', () => {
  const { container } = render(<IrViewer before={before} after={null} />)
  expect(screen.getByRole('heading', { name: 'Before' })).toBeInTheDocument()
  expect(screen.getByRole('heading', { name: 'After' })).toBeInTheDocument()
  expect(screen.getByText('No after snapshot')).toBeInTheDocument()
  expect(container.querySelectorAll('.cm-editor')).toHaveLength(1)
})

test('selects the narrowest operation covering a clicked text line', () => {
  const onSelectOp = vi.fn()
  const operations: SelectableOp[] = [
    {
      uid: 'outer',
      op_idx: 0,
      name: 'module',
      line_start: 1,
      line_end: 3,
    },
    {
      uid: 'inner',
      op_idx: 1,
      name: 'arith.constant',
      line_start: 1,
      line_end: 1,
    },
  ]
  const { container } = render(
    <IrViewer
      before={before}
      after={null}
      beforeOps={operations}
      afterOps={[]}
      onSelectOp={onSelectOp}
    />,
  )

  fireEvent.mouseDown(container.querySelector('.cm-line')!)
  expect(onSelectOp).toHaveBeenCalledWith('inner')
})
