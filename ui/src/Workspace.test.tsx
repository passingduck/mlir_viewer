import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useViewerStore } from './store'
import { resetLayout, Workspace } from './Workspace'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>()
  return {
    ...actual,
    api: {
      traceInfo: vi.fn(async () => ({ format_version: '1', pass_count: 1, meta: {} })),
      passes: vi.fn(async () => [
        {
          id: 1,
          name: 'p',
          ir_before: 10,
          ir_after: 11,
          start_ns: 0,
          end_ns: 1,
          ir_changed: true,
          children: [],
        },
      ]),
      irPage: vi.fn(async (_passId: number, side: 'before' | 'after') => ({
        pass_id: 1,
        side,
        text: 'x',
        offset: 0,
        next_offset: null,
        total_bytes: 1,
      })),
      functions: vi.fn(async () => [
        { name: 'forward', op_count: 3, has_before: true, has_after: true },
      ]),
      diff: vi.fn(async () => ({ func: 'forward', changes: [] })),
      graph: vi.fn(async () => ({ nodes: [], edges: [], clusters: [], truncated: false })),
      selectableOps: vi.fn(async () => []),
      opHistory: vi.fn(async (uid: string) => ({
        uid,
        first_name: 'op',
        last_name: 'op',
        steps: [],
      })),
      opDetail: vi.fn(async (uid: string) => ({
        uid,
        func: 'forward',
        pass_id: 1,
        side: 'before',
        op_idx: 0,
        name: 'op',
        results: [],
        operands: [],
        result_types: [],
        attr_summary: '',
        location: null,
        region_path: [],
        line_start: 1,
        line_end: 1,
        opaque: false,
      })),
      searchOps: vi.fn(async () => []),
    },
  }
})

beforeEach(async () => {
  localStorage.clear()
  vi.clearAllMocks()
  useViewerStore.getState().reset()
  await useViewerStore.getState().load()
})

afterEach(() => {
  localStorage.clear()
  cleanup()
})

describe('Workspace', () => {
  it('persists layout to localStorage and resets it', async () => {
    render(<Workspace />)
    await screen.findByLabelText('Pass timeline')
    await waitFor(() =>
      expect(localStorage.getItem('mlir-viewer-layout-v1')).toBeTruthy(),
    )
    resetLayout()
    expect(localStorage.getItem('mlir-viewer-layout-v1')).toBeNull()
  })
})
