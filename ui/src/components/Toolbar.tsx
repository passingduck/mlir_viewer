import { useEffect } from 'react'
import { useViewerStore } from '../store'

interface ToolbarProps {
  diffAvailable: boolean
}

export function Toolbar({ diffAvailable }: ToolbarProps) {
  const {
    viewMode,
    diffEnabled,
    functions,
    selectedFunc,
    selectedOpUid,
    setViewMode,
    toggleDiff,
    selectFunc,
    openInspector,
  } = useViewerStore()

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      if (target && (target.tagName === 'INPUT' || target.tagName === 'SELECT')) return
      if (event.key === 't') setViewMode('text')
      else if (event.key === 'g') setViewMode('graph')
      else if (event.key === 'h' && selectedOpUid) openInspector('history')
      else if (event.key === 'd' && diffAvailable) toggleDiff()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [diffAvailable, selectedOpUid, setViewMode, toggleDiff, openInspector])

  return (
    <div className="toolbar" role="toolbar" aria-label="View controls">
      <div className="segmented" role="group" aria-label="View mode">
        <button
          type="button"
          aria-pressed={viewMode === 'text'}
          className={viewMode === 'text' ? 'active' : ''}
          onClick={() => setViewMode('text')}
        >
          Text
        </button>
        <button
          type="button"
          aria-pressed={viewMode === 'graph'}
          className={viewMode === 'graph' ? 'active' : ''}
          onClick={() => setViewMode('graph')}
        >
          Graph
        </button>
      </div>
      <button
        type="button"
        className={diffEnabled ? 'diff-toggle active' : 'diff-toggle'}
        aria-pressed={diffEnabled}
        disabled={!diffAvailable}
        title={diffAvailable ? 'Toggle structural diff (d)' : 'This pass is missing a before or after snapshot'}
        onClick={toggleDiff}
      >
        Diff
      </button>
      {viewMode === 'graph' && functions.length > 1 && (
        <select
          aria-label="Function"
          value={selectedFunc ?? ''}
          onChange={(event) => selectFunc(event.target.value)}
        >
          {functions.map((func) => (
            <option key={func.name} value={func.name}>
              {func.name} ({func.op_count})
            </option>
          ))}
        </select>
      )}
    </div>
  )
}
