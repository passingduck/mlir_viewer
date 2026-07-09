import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it } from 'vitest'
import { useViewerStore } from '../store'
import { InspectorPanel } from './InspectorPanel'

const detail = {
  uid: 'op1.Zg.2.b.0',
  func: 'f',
  pass_id: 2,
  side: 'before' as const,
  op_idx: 0,
  name: 'arith.addi',
  results: ['%r'],
  operands: ['%a', '%b'],
  result_types: ['i32'],
  attr_summary: '{fast}',
  location: 'unknown',
  region_path: [0, 0],
  line_start: 3,
  line_end: 3,
  opaque: false,
}

beforeEach(() => {
  useViewerStore.setState({
    ...useViewerStore.getState(),
    inspectorOpen: true,
    inspectorTab: 'structure',
    selectedOpUid: detail.uid,
    opDetail: detail,
    history: null,
    detailStale: false,
  })
})
afterEach(cleanup)

it('renders structure fields and switches to history tab', () => {
  render(<InspectorPanel />)
  expect(screen.getByText('arith.addi')).toBeInTheDocument()
  expect(screen.getByText('%a')).toBeInTheDocument()
  expect(screen.getByText('i32')).toBeInTheDocument()
  expect(screen.getByText('{fast}')).toBeInTheDocument()
  fireEvent.click(screen.getByRole('tab', { name: 'History' }))
  expect(useViewerStore.getState().inspectorTab).toBe('history')
})

it('close button clears inspectorOpen', () => {
  render(<InspectorPanel />)
  fireEvent.click(screen.getByRole('button', { name: 'Close inspector' }))
  expect(useViewerStore.getState().inspectorOpen).toBe(false)
})
