import { create } from 'zustand'
import {
  api,
  type DataflowGraph,
  type FunctionDiff,
  type FunctionInfo,
  type IrPage,
  type IrSide,
  type PassNode,
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
  load: () => Promise<void>
  selectPass: (id: number) => Promise<void>
  setViewMode: (mode: ViewMode) => void
  toggleDiff: () => void
  selectFunc: (name: string) => void
  refreshView: () => Promise<void>
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
    set({ selectedPassId: id, before: null, after: null, diff: null, graph: null, error: null })
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
    set({ selectedFunc })
    void get().refreshView()
  },
  refreshView: async () => {
    const { selectedPassId, selectedFunc, viewMode, diffEnabled } = get()
    if (selectedPassId === null || selectedFunc === null) return

    try {
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
  reset: () => set(initialState),
}))
