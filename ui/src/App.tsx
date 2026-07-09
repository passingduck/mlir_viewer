import { useEffect } from 'react'
import { CommandPalette } from './components/CommandPalette'
import { Workspace } from './Workspace'
import { useGlobalKeys } from './useGlobalKeys'
import { useViewerStore } from './store'
import './styles.css'

export function App() {
  const { status, error, info, roots, load } = useViewerStore()
  useGlobalKeys()
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
      {status === 'ready' && roots.length > 0 && <Workspace />}
      {error && status !== 'error' && <div className="toast" role="alert">{error}</div>}
      <CommandPalette />
    </div>
  )
}
