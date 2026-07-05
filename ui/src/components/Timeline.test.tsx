import { fireEvent, render, screen } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import type { PassNode } from '../api'
import { Timeline } from './Timeline'

const roots: PassNode[] = [
  {
    id: 1,
    name: 'Pipeline',
    ir_before: 1,
    ir_after: 2,
    start_ns: 0,
    end_ns: 2_000_000,
    ir_changed: true,
    children: [
      { id: 2, name: 'canonicalize', ir_before: 1, ir_after: 2, start_ns: 0, end_ns: 1_000_000, ir_changed: true, children: [] },
      { id: 3, name: 'cse', ir_before: 2, ir_after: 2, start_ns: 1_000_000, end_ns: 1_500_000, ir_changed: false, children: [] },
    ],
  },
]

test('renders nested timing and badges and supports click and step keys', () => {
  const onSelect = vi.fn()
  const { rerender } = render(
    <Timeline roots={roots} selectedPassId={2} onSelect={onSelect} />,
  )
  expect(screen.getByText('1.00 ms')).toBeInTheDocument()
  expect(screen.getByText('no-op')).toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: /cse/ }))
  expect(onSelect).toHaveBeenCalledWith(3)

  rerender(<Timeline roots={roots} selectedPassId={2} onSelect={onSelect} />)
  fireEvent.keyDown(window, { key: ']' })
  expect(onSelect).toHaveBeenLastCalledWith(3)
  fireEvent.keyDown(window, { key: '[' })
  expect(onSelect).toHaveBeenLastCalledWith(1)
})
