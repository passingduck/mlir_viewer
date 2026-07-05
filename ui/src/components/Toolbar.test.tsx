import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { useViewerStore } from '../store'
import { Toolbar } from './Toolbar'

beforeEach(() => {
  useViewerStore.setState({
    ...useViewerStore.getState(),
    viewMode: 'text',
    diffEnabled: false,
    functions: [{ name: 'forward', op_count: 3, has_before: true, has_after: true }],
    selectedFunc: 'forward',
    selectedPassId: 1,
    selectedOpUid: null,
    history: null,
  })
})

afterEach(cleanup)

describe('Toolbar', () => {
  it('switches view mode on click', () => {
    render(<Toolbar diffAvailable />)
    fireEvent.click(screen.getByRole('button', { name: 'Graph' }))
    expect(useViewerStore.getState().viewMode).toBe('graph')
  })

  it('disables diff when unavailable', () => {
    render(<Toolbar diffAvailable={false} />)
    expect(screen.getByRole('button', { name: /Diff/ })).toBeDisabled()
  })

  it('keyboard g switches to graph, t back to text', () => {
    render(<Toolbar diffAvailable />)
    fireEvent.keyDown(window, { key: 'g' })
    expect(useViewerStore.getState().viewMode).toBe('graph')
    fireEvent.keyDown(window, { key: 't' })
    expect(useViewerStore.getState().viewMode).toBe('text')
  })

  it('enables History only after op selection and supports keyboard h', () => {
    const { rerender } = render(<Toolbar diffAvailable />)
    expect(screen.getByRole('button', { name: 'History' })).toBeDisabled()

    useViewerStore.setState({ selectedOpUid: 'op1.Zg.1.b.0' })
    rerender(<Toolbar diffAvailable />)
    fireEvent.keyDown(window, { key: 'h' })
    expect(useViewerStore.getState().viewMode).toBe('history')
  })
})
