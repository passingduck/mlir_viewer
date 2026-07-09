import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from 'dockview-react'
import { useEffect, useRef } from 'react'
import { Timeline } from './components/Timeline'
import { Toolbar } from './components/Toolbar'
import { IrViewer } from './components/IrViewer'
import { GraphView } from './components/GraphView'
import { InspectorPanel } from './components/InspectorPanel'
import { useViewerStore } from './store'

const LAYOUT_KEY = 'mlir-viewer-layout-v1'
let dockApi: DockviewApi | null = null

export function resetLayout() {
  localStorage.removeItem(LAYOUT_KEY)
  if (dockApi) buildDefaultLayout(dockApi)
}

function buildDefaultLayout(api: DockviewApi) {
  api.clear()
  api.addPanel({ id: 'timeline', component: 'timeline', title: 'Timeline' })
  api.addPanel({
    id: 'viewer',
    component: 'viewer',
    title: 'IR',
    position: { referencePanel: 'timeline', direction: 'right' },
  })
  api.getPanel('timeline')?.api.setSize({ width: 280 })
}

function TimelinePanel(_: IDockviewPanelProps) {
  const { roots, selectedPassId, selectPass } = useViewerStore()
  return (
    <nav aria-label="Pass timeline" className="panel-scroll">
      <Timeline roots={roots} selectedPassId={selectedPassId} onSelect={(id) => void selectPass(id)} />
    </nav>
  )
}

function ViewerPanel(_: IDockviewPanelProps) {
  const {
    passesById,
    selectedPassId,
    before,
    after,
    diff,
    graph,
    diffEnabled,
    viewMode,
    selectableBefore,
    selectableAfter,
    selectOp,
  } = useViewerStore()
  const selectedPass = selectedPassId === null ? null : passesById[selectedPassId]
  const diffAvailable = Boolean(
    selectedPass && selectedPass.ir_before !== null && selectedPass.ir_after !== null,
  )
  return (
    <div className="viewer-pane">
      <Toolbar diffAvailable={diffAvailable} />
      {viewMode === 'graph' ? (
        <GraphView graph={graph} diffEnabled={diffEnabled} onSelectOp={selectOp} />
      ) : (
        <IrViewer
          before={before}
          after={after}
          diff={diffEnabled ? diff : null}
          beforeOps={selectableBefore}
          afterOps={selectableAfter}
          onSelectOp={selectOp}
        />
      )}
    </div>
  )
}

function InspectorDockPanel(_: IDockviewPanelProps) {
  return <InspectorPanel />
}

const components = { timeline: TimelinePanel, viewer: ViewerPanel, inspector: InspectorDockPanel }

export function Workspace() {
  const inspectorOpen = useViewerStore((state) => state.inspectorOpen)
  const saveTimer = useRef<number | undefined>(undefined)

  const onReady = (event: DockviewReadyEvent) => {
    dockApi = event.api
    const saved = localStorage.getItem(LAYOUT_KEY)
    let restored = false
    if (saved) {
      try {
        event.api.fromJSON(JSON.parse(saved))
        restored = true
      } catch {
        localStorage.removeItem(LAYOUT_KEY)
      }
    }
    if (!restored) buildDefaultLayout(event.api)
    const scheduleSave = () => {
      window.clearTimeout(saveTimer.current)
      saveTimer.current = window.setTimeout(() => {
        localStorage.setItem(LAYOUT_KEY, JSON.stringify(event.api.toJSON()))
      }, 250)
    }
    event.api.onDidLayoutChange(scheduleSave)
    event.api.onDidAddPanel(scheduleSave)
    event.api.onDidRemovePanel((panel) => {
      if (panel.id === 'inspector') useViewerStore.getState().closeInspector()
      else scheduleSave()
    })
    // Persist the initial layout (default or restored) so a reload restores it
    // even when no drag/resize event has fired yet.
    scheduleSave()
  }

  useEffect(() => {
    if (!dockApi) return
    const existing = dockApi.getPanel('inspector')
    if (inspectorOpen && !existing) {
      dockApi.addPanel({
        id: 'inspector',
        component: 'inspector',
        title: 'Inspector',
        position: { referencePanel: 'viewer', direction: 'right' },
      })
      dockApi.getPanel('inspector')?.api.setSize({ width: 340 })
    } else if (!inspectorOpen && existing) {
      dockApi.removePanel(existing)
    }
  }, [inspectorOpen])

  return <DockviewReact className="dockview-theme-dark workspace" components={components} onReady={onReady} />
}
