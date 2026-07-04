import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, expect, test, vi } from 'vitest'
import { App } from './App'
import { useViewerStore } from './store'

const info = {
  format_version: '1',
  pass_count: 2,
  meta: { producer: 'test' },
}

const passes = [
  {
    id: 1,
    name: 'Pipeline',
    ir_before: 1,
    ir_after: 2,
    start_ns: 0,
    end_ns: 2_000_000,
    ir_changed: true,
    children: [
      {
        id: 2,
        name: 'canonicalize',
        ir_before: 1,
        ir_after: 2,
        start_ns: 0,
        end_ns: 1_000_000,
        ir_changed: true,
        children: [],
      },
    ],
  },
]

beforeEach(() => {
  useViewerStore.getState().reset()
  vi.stubGlobal(
    'fetch',
    vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(info)))
      .mockResolvedValueOnce(new Response(JSON.stringify(passes)))
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ pass_id: 2, side: 'before', text: 'before', offset: 0, next_offset: null, total_bytes: 6 })),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ pass_id: 2, side: 'after', text: 'after', offset: 0, next_offset: null, total_bytes: 5 })),
      ),
  )
})

test('loads the trace, selects the first changed leaf pass, and fetches both sides', async () => {
  render(<App />)
  expect(screen.getByText('Loading trace…')).toBeInTheDocument()

  expect(await screen.findByText('canonicalize')).toBeInTheDocument()
  await waitFor(() => expect(useViewerStore.getState().selectedPassId).toBe(2))
  await waitFor(() => expect(useViewerStore.getState().before?.text).toBe('before'))
  expect(useViewerStore.getState().after?.text).toBe('after')
})
