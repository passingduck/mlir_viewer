import { create } from 'zustand'
import { api, type IrPage, type IrSide, type PassNode, type TraceInfo } from './api'

type LoadState = 'idle' | 'loading' | 'ready' | 'error'

interface ViewerState {
  status: LoadState
  error: string | null
  info: TraceInfo | null
  roots: PassNode[]
  passesById: Record<number, PassNode>
  selectedPassId: number | null
  before: IrPage | null
  after: IrPage | null
  load: () => Promise<void>
  selectPass: (id: number) => Promise<void>
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
    set({ selectedPassId: id, before: null, after: null, error: null })
    try {
      const [before, after] = await Promise.all([
        loadSide(pass, 'before'),
        loadSide(pass, 'after'),
      ])
      if (get().selectedPassId === id) set({ before, after })
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  },
  reset: () => set(initialState),
}))
