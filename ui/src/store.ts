import { create } from 'zustand'
import {
  api,
  ApiError,
  type DataflowGraph,
  type FunctionDiff,
  type FunctionInfo,
  type IrPage,
  type IrSide,
  type OpDetail,
  type OpHistory,
  type PassNode,
  type SearchResult,
  type SelectableOp,
  type TraceInfo,
} from './api'

type LoadState = 'idle' | 'loading' | 'ready' | 'error'
type ViewMode = 'text' | 'graph'

const GRAPH_BUDGET = 2000

interface ViewerState {
  status: LoadState
  error: string | null
  info: TraceInfo | null
  roots: PassNode[]
  passesById: Record<number, PassNode>
  selectedPassId: number | null
  before: IrPage | null
  after: IrPage | null
  viewMode: ViewMode
  diffEnabled: boolean
  selectedFunc: string | null
  functions: FunctionInfo[]
  diff: FunctionDiff | null
  graph: DataflowGraph | null
  selectableBefore: SelectableOp[]
  selectableAfter: SelectableOp[]
  selectedOpUid: string | null
  history: OpHistory | null
  inspectorOpen: boolean
  inspectorTab: 'structure' | 'history'
  opDetail: OpDetail | null
  detailStale: boolean
  paletteOpen: boolean
  load: () => Promise<void>
  selectPass: (id: number) => Promise<void>
  setViewMode: (mode: ViewMode) => void
  toggleDiff: () => void
  selectFunc: (name: string) => void
  refreshView: () => Promise<void>
  selectOp: (uid: string) => Promise<void>
  openInspector: (tab: 'structure' | 'history') => void
  closeInspector: () => void
  setPaletteOpen: (open: boolean) => void
  stepPass: (direction: 1 | -1) => Promise<void>
  jumpToSearchResult: (result: SearchResult) => Promise<void>
  viewHistoryStep: (passId: number) => Promise<void>
  reset: () => void
}

const initialState = {
  status: 'idle' as LoadState,
  error: null,
  info: null,
  roots: [] as PassNode[],
  passesById: {} as Record<number, PassNode>,
  selectedPassId: null,
  before: null,
  after: null,
  viewMode: 'text' as ViewMode,
  diffEnabled: false,
  selectedFunc: null as string | null,
  functions: [] as FunctionInfo[],
  diff: null as FunctionDiff | null,
  graph: null as DataflowGraph | null,
  selectableBefore: [] as SelectableOp[],
  selectableAfter: [] as SelectableOp[],
  selectedOpUid: null as string | null,
  history: null as OpHistory | null,
  inspectorOpen: false,
  inspectorTab: 'structure' as const,
  opDetail: null as OpDetail | null,
  detailStale: false,
  paletteOpen: false,
}

function flatten(nodes: PassNode[], output: PassNode[] = []): PassNode[] {
  for (const node of nodes) {
    output.push(node)
    flatten(node.children, output)
  }
  return output
}

async function loadSide(pass: PassNode, side: IrSide): Promise<IrPage | null> {
  const blob = side === 'before' ? pass.ir_before : pass.ir_after
  return blob === null ? null : api.irPage(pass.id, side)
}

export const useViewerStore = create<ViewerState>((set, get) => ({
  ...initialState,
  load: async () => {
    set({ ...initialState, status: 'loading' })
    try {
      const [info, roots] = await Promise.all([api.traceInfo(), api.passes()])
      const ordered = flatten(roots)
      const passesById = Object.fromEntries(ordered.map((pass) => [pass.id, pass]))
      const selected =
        ordered.find((pass) => pass.children.length === 0 && pass.ir_changed) ??
        ordered.find((pass) => pass.ir_changed) ??
        ordered[0]
      set({ info, roots, passesById, status: 'ready' })
      if (selected) await get().selectPass(selected.id)
    } catch (error) {
      set({ status: 'error', error: error instanceof Error ? error.message : String(error) })
    }
  },
  selectPass: async (id) => {
    const pass = get().passesById[id]
    if (!pass) return
    set({
      selectedPassId: id,
      before: null,
      after: null,
      diff: null,
      graph: null,
      selectableBefore: [],
      selectableAfter: [],
      error: null,
    })
    try {
      const [before, after, functions] = await Promise.all([
        loadSide(pass, 'before'),
        loadSide(pass, 'after'),
        api.functions(id),
      ])
      if (get().selectedPassId !== id) return
      const previous = get().selectedFunc
      const selectedFunc =
        previous && functions.some((func) => func.name === previous)
          ? previous
          : (functions[0]?.name ?? null)
      set({ before, after, functions, selectedFunc })
      await get().refreshView()
      // Keep the inspector detail in sync with the pass the user is viewing.
      const uid = get().selectedOpUid
      if (uid) {
        try {
          const detail = await api.opDetail(uid, id)
          if (get().selectedOpUid === uid) set({ opDetail: detail, detailStale: false })
        } catch (error) {
          if (get().selectedOpUid === uid) {
            if (error instanceof ApiError && error.status === 404) {
              set({ detailStale: true })
            } else {
              set({ error: error instanceof Error ? error.message : String(error) })
            }
          }
        }
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  },
  setViewMode: (viewMode) => {
    set({ viewMode })
    void get().refreshView()
  },
  toggleDiff: () => {
    set({ diffEnabled: !get().diffEnabled })
    void get().refreshView()
  },
  selectFunc: (selectedFunc) => {
    set({
      selectedFunc,
      selectedOpUid: null,
      history: null,
      opDetail: null,
      inspectorOpen: false,
      detailStale: false,
    })
    void get().refreshView()
  },
  refreshView: async () => {
    const { selectedPassId, selectedFunc, viewMode, diffEnabled, passesById } = get()
    if (selectedPassId === null || selectedFunc === null) return

    try {
      const pass = passesById[selectedPassId]
      const [selectableBefore, selectableAfter] = await Promise.all([
        pass?.ir_before === null
          ? Promise.resolve([])
          : api.selectableOps(selectedPassId, 'before', selectedFunc),
        pass?.ir_after === null
          ? Promise.resolve([])
          : api.selectableOps(selectedPassId, 'after', selectedFunc),
      ])
      if (get().selectedPassId === selectedPassId && get().selectedFunc === selectedFunc) {
        set({ selectableBefore, selectableAfter })
      }
      if (viewMode === 'graph') {
        const graph = await api.graph(selectedPassId, selectedFunc, diffEnabled, GRAPH_BUDGET)
        if (get().selectedPassId === selectedPassId && get().selectedFunc === selectedFunc) {
          set({ graph })
        }
      } else if (diffEnabled) {
        const diff = await api.diff(selectedPassId, selectedFunc)
        if (get().selectedPassId === selectedPassId && get().selectedFunc === selectedFunc) {
          set({ diff })
        }
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  },
  selectOp: async (uid) => {
    set({
      selectedOpUid: uid,
      history: null,
      opDetail: null,
      detailStale: false,
      inspectorOpen: true,
      error: null,
    })
    const passId = get().selectedPassId
    try {
      const [history, detail] = await Promise.all([
        api.opHistory(uid),
        api.opDetail(uid, passId ?? undefined),
      ])
      if (get().selectedOpUid === uid) set({ history, opDetail: detail })
    } catch (error) {
      if (get().selectedOpUid === uid) {
        set({ error: error instanceof Error ? error.message : String(error) })
      }
    }
  },
  openInspector: (tab) => set({ inspectorOpen: true, inspectorTab: tab }),
  closeInspector: () => set({ inspectorOpen: false }),
  setPaletteOpen: (open) => set({ paletteOpen: open }),
  stepPass: async (direction) => {
    const { roots, selectedPassId } = get()
    const leaves: PassNode[] = []
    const walk = (nodes: PassNode[]) => {
      for (const node of nodes) {
        if (node.children.length === 0) leaves.push(node)
        else walk(node.children)
      }
    }
    walk(roots)
    const index = leaves.findIndex((leaf) => leaf.id === selectedPassId)
    const next =
      leaves[index === -1 ? 0 : Math.min(Math.max(index + direction, 0), leaves.length - 1)]
    if (next && next.id !== selectedPassId) await get().selectPass(next.id)
  },
  jumpToSearchResult: async (result) => {
    await get().selectPass(result.pass_id)
    if (get().selectedFunc !== result.func) await get().selectFunc(result.func)
    const ops = await api.selectableOps(result.pass_id, result.side, result.func)
    const op = ops.find((candidate) => candidate.op_idx === result.op_idx)
    if (op) await get().selectOp(op.uid)
  },
  viewHistoryStep: async (passId) => {
    await get().selectPass(passId)
    set({ viewMode: 'text' })
  },
  reset: () => set(initialState),
}))
