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
      selectableOps: vi.fn(async (_passId: number, side: 'before' | 'after') => [
        {
          uid: `op1.Zg.1.${side === 'before' ? 'b' : 'a'}.0`,
          op_idx: 0,
          name: 'test.op',
          line_start: 1,
          line_end: 1,
        },
      ]),
      opHistory: vi.fn(async (uid: string) => ({
        uid,
        first_name: 'test.op',
        last_name: 'test.op',
        steps: [],
      })),
      opDetail: vi.fn(async (uid: string) => ({
        uid,
        func: 'forward',
        pass_id: 1,
        side: 'before',
        op_idx: 0,
        name: 'test.op',
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
    },
  }
})

beforeEach(() => {
  vi.clearAllMocks()
  useViewerStore.getState().reset()
})

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

  it('loads selectable operations for both snapshot sides', async () => {
    await useViewerStore.getState().load()

    expect(api.selectableOps).toHaveBeenCalledTimes(2)
    expect(useViewerStore.getState().selectableBefore).toHaveLength(1)
    expect(useViewerStore.getState().selectableAfter).toHaveLength(1)
  })

  it('selects an op, opens inspector, and preserves selection when viewing IR', async () => {
    await useViewerStore.getState().load()
    await useViewerStore.getState().selectOp('op1.Zg.1.b.0')

    expect(useViewerStore.getState().inspectorOpen).toBe(true)
    expect(useViewerStore.getState().opDetail?.uid).toBe('op1.Zg.1.b.0')

    await useViewerStore.getState().viewHistoryStep(1)
    expect(useViewerStore.getState().viewMode).toBe('text')
    expect(useViewerStore.getState().selectedOpUid).toBe('op1.Zg.1.b.0')
  })

  it('stepPass walks executable leaves in order and clamps at the ends', async () => {
    vi.mocked(api.passes).mockImplementationOnce(async () => [
      {
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
          {
            id: 4,
            name: 'set-attr',
            ir_before: 12,
            ir_after: 13,
            start_ns: 2,
            end_ns: 3,
            ir_changed: true,
            children: [],
          },
        ],
      },
    ])
    await useViewerStore.getState().load()
    useViewerStore.setState({ selectedPassId: 2 })
    await useViewerStore.getState().stepPass(1)
    expect(useViewerStore.getState().selectedPassId).toBe(3)
    await useViewerStore.getState().stepPass(-1)
    await useViewerStore.getState().stepPass(-1)
    expect(useViewerStore.getState().selectedPassId).toBe(2)
    await useViewerStore.getState().stepPass(1)
    await useViewerStore.getState().stepPass(1)
    expect(useViewerStore.getState().selectedPassId).toBe(4)
  })
})
