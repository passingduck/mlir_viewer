import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from './api'
import { useViewerStore } from './store'

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
    },
  }
})

beforeEach(() => useViewerStore.getState().reset())

describe('store toggles', () => {
  it('defaults to text mode, diff off', () => {
    const state = useViewerStore.getState()
    expect(state.viewMode).toBe('text')
    expect(state.diffEnabled).toBe(false)
  })

  it('viewMode and diffEnabled survive pass selection', async () => {
    await useViewerStore.getState().load()
    useViewerStore.getState().setViewMode('graph')
    useViewerStore.getState().toggleDiff()
    await useViewerStore.getState().selectPass(1)

    expect(useViewerStore.getState().viewMode).toBe('graph')
    expect(useViewerStore.getState().diffEnabled).toBe(true)
  })

  it('loads functions and defaults selectedFunc to first', async () => {
    await useViewerStore.getState().load()

    expect(useViewerStore.getState().functions.map((func) => func.name)).toEqual(['forward'])
    expect(useViewerStore.getState().selectedFunc).toBe('forward')
  })

  it('fetches diff when diff enabled in text mode', async () => {
    await useViewerStore.getState().load()
    useViewerStore.getState().toggleDiff()
    await useViewerStore.getState().selectPass(1)

    expect(api.diff).toHaveBeenCalled()
    expect(useViewerStore.getState().diff).not.toBeNull()
  })
})
