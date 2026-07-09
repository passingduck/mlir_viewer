import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { api, type SearchResult } from '../api'
import { useViewerStore } from '../store'
import { CommandPalette } from './CommandPalette'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      searchOps: vi.fn(async (_query: string, _passId: number) => [] as SearchResult[]),
    },
  }
})

const demoPass = {
  id: 1,
  name: 'p',
  ir_before: 10,
  ir_after: 11,
  start_ns: 0,
  end_ns: 1,
  ir_changed: true,
  children: [
    {
      id: 2,
      name: 'canonicalize',
      ir_before: 10,
      ir_after: 11,
      start_ns: 0,
      end_ns: 1,
      ir_changed: true,
      children: [],
    },
    {
      id: 3,
      name: 'dce',
      ir_before: 11,
      ir_after: 12,
      start_ns: 1,
      end_ns: 2,
      ir_changed: true,
      children: [],
    },
  ],
}

beforeEach(() => {
  useViewerStore.setState({
    ...useViewerStore.getState(),
    paletteOpen: true,
    roots: [demoPass] as never,
    functions: [{ name: 'forward', op_count: 3, has_before: true, has_after: true }],
    selectedPassId: 2,
  })
})
afterEach(cleanup)

describe('CommandPalette', () => {
  it('lists passes and runs actions', async () => {
    render(<CommandPalette />)
    fireEvent.change(screen.getByPlaceholderText('Search passes, functions, ops…'), {
      target: { value: 'canonic' },
    })
    expect(await screen.findByText('canonicalize')).toBeInTheDocument()
    fireEvent.click(screen.getByText('canonicalize'))
    expect(useViewerStore.getState().selectedPassId).toBe(2)
    expect(useViewerStore.getState().paletteOpen).toBe(false)
  })
})
