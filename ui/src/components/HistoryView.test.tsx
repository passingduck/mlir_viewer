import { fireEvent, render, screen } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import type { OpHistory, OpOccurrence } from '../api'
import { HistoryView } from './HistoryView'

const before = (op_idx: number, name: string): OpOccurrence => ({
  side: 'before',
  op_idx,
  name,
  line_start: op_idx + 1,
  line_end: op_idx + 1,
  attr_summary: '',
  location: null,
})

const after = (op_idx: number, name: string): OpOccurrence => ({
  ...before(op_idx, name),
  side: 'after',
})

const history: OpHistory = {
  uid: 'op1.Zg.1.b.0',
  first_name: 'arith.addi',
  last_name: 'arith.shli',
  steps: [
    {
      pass_id: 1,
      pass_name: 'canonicalize',
      change: 'replaced',
      before: before(1, 'arith.addi'),
      after: after(2, 'arith.shli'),
      evidence: [{ seq: 0, pattern: 'AddIToShift', source: 'action' }],
      confidence: { kind: 'exact' },
    },
    {
      pass_id: 2,
      pass_name: 'unknown-pass',
      change: 'modified',
      before: before(2, 'arith.shli'),
      after: after(2, 'arith.shli'),
      evidence: [{ seq: 0, pattern: null, source: 'fingerprint' }],
      confidence: { kind: 'inferred', score: 75 },
    },
    {
      pass_id: 3,
      pass_name: 'cse',
      change: 'replaced',
      before: before(3, 'arith.addi'),
      after: after(5, 'arith.shli'),
      evidence: [{ seq: 0, pattern: 'CSE', source: 'listener' }],
      confidence: { kind: 'exact' },
    },
    {
      pass_id: 3,
      pass_name: 'cse',
      change: 'replaced',
      before: before(4, 'arith.addi'),
      after: after(5, 'arith.shli'),
      evidence: [{ seq: 1, pattern: 'CSE', source: 'listener' }],
      confidence: { kind: 'exact' },
    },
  ],
}

test('renders exact, inferred, evidence, and merged predecessor steps', () => {
  const onViewIr = vi.fn()
  const { container } = render(<HistoryView history={history} onViewIr={onViewIr} />)

  expect(container.querySelectorAll('.history-step.exact')).toHaveLength(3)
  expect(container.querySelectorAll('.history-step.inferred')).toHaveLength(1)
  expect(screen.getByText('75% inferred')).toBeInTheDocument()
  expect(screen.getByText('AddIToShift')).toBeInTheDocument()
  expect(screen.getAllByText('listener')).toHaveLength(2)
  expect(screen.getByText('2 predecessors merge')).toBeInTheDocument()

  fireEvent.click(screen.getAllByRole('button', { name: 'View IR' })[0])
  expect(onViewIr).toHaveBeenCalledWith(1)
})

test('renders a loading state before history arrives', () => {
  render(<HistoryView history={null} onViewIr={() => {}} />)
  expect(screen.getByText('Loading operation history…')).toBeInTheDocument()
})

test('renders a disappeared terminal step', () => {
  render(
    <HistoryView
      history={{
        uid: 'op1.Zg.1.b.0',
        first_name: 'x.vanish',
        last_name: 'x.vanish',
        steps: [
          {
            pass_id: 1,
            pass_name: 'canonicalize',
            change: 'disappeared',
            before: { side: 'before', op_idx: 0, name: 'x.vanish', line_start: 2, line_end: 2, attr_summary: '', location: null },
            after: null,
            evidence: [],
            confidence: { kind: 'exact' },
          },
        ],
      }}
      onViewIr={() => {}}
    />,
  )
  expect(screen.getByText('disappeared')).toBeInTheDocument()
})
