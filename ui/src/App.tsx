import { useEffect } from 'react'
import { IrViewer } from './components/IrViewer'
import { Timeline } from './components/Timeline'
import { Toolbar } from './components/Toolbar'
import { useViewerStore } from './store'
import './styles.css'

export function App() {
  const { status, error, info, roots, passesById, selectedPassId, before, after, load, selectPass } = useViewerStore()
  const selectedPass = selectedPassId === null ? null : passesById[selectedPassId]
  const diffAvailable = Boolean(
    selectedPass && selectedPass.ir_before !== null && selectedPass.ir_after !== null,
  )
  useEffect(() => {
    void load()
  }, [load])

  return (
    <div className="app-shell">
      <header>
        <strong>MLIR Viewer</strong>
        <span>{info ? `${info.pass_count} passes · format v${info.format_version}` : 'Trace'}</span>
      </header>
      {status === 'loading' && <div className="status">Loading trace…</div>}
      {status === 'error' && <div className="status error" role="alert">{error}</div>}
      {status === 'ready' && roots.length === 0 && <div className="status">No passes recorded.</div>}
      {status === 'ready' && roots.length > 0 && (
        <main>
          <nav aria-label="Pass timeline">
            <Timeline roots={roots} selectedPassId={selectedPassId} onSelect={(id) => void selectPass(id)} />
          </nav>
          <div className="viewer-pane">
            <Toolbar diffAvailable={diffAvailable} />
            <IrViewer before={before} after={after} />
          </div>
        </main>
      )}
      {error && status !== 'error' && <div className="toast" role="alert">{error}</div>}
    </div>
  )
}
