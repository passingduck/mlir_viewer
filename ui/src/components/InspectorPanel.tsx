import { useViewerStore } from '../store'
import { HistoryView } from './HistoryView'

export function InspectorPanel() {
  const { inspectorTab, opDetail, detailStale, history, openInspector, closeInspector, viewHistoryStep } =
    useViewerStore()
  return (
    <aside className="inspector" aria-label="Operation inspector">
      <header className="inspector-header">
        <div role="tablist" aria-label="Inspector tabs">
          <button
            type="button"
            role="tab"
            aria-selected={inspectorTab === 'structure'}
            onClick={() => openInspector('structure')}
          >
            Structure
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={inspectorTab === 'history'}
            onClick={() => openInspector('history')}
          >
            History
          </button>
        </div>
        <button type="button" aria-label="Close inspector" onClick={closeInspector}>
          ×
        </button>
      </header>
      {inspectorTab === 'history' ? (
        <HistoryView history={history} onViewIr={viewHistoryStep} />
      ) : !opDetail ? (
        <div className="status">Loading operation…</div>
      ) : (
        <dl className="op-structure">
          {detailStale && (
            <div className="status">Not present in this pass — showing last known occurrence.</div>
          )}
          <dt>Operation</dt>
          <dd>
            <code>{opDetail.name}</code>
          </dd>
          {opDetail.results.length > 0 && (
            <>
              <dt>Results</dt>
              <dd>{opDetail.results.map((result) => <code key={result}>{result}</code>)}</dd>
            </>
          )}
          {opDetail.operands.length > 0 && (
            <>
              <dt>Operands</dt>
              <dd>{opDetail.operands.map((operand) => <code key={operand}>{operand}</code>)}</dd>
            </>
          )}
          {opDetail.result_types.length > 0 && (
            <>
              <dt>Types</dt>
              <dd>
                {opDetail.result_types.map((type) => <code key={type}>{type}</code>)}
              </dd>
            </>
          )}
          {opDetail.attr_summary && (
            <>
              <dt>Attributes</dt>
              <dd>
                <code className="attrs">{opDetail.attr_summary}</code>
              </dd>
            </>
          )}
          {opDetail.location && (
            <>
              <dt>Location</dt>
              <dd>
                <code>{opDetail.location}</code>
              </dd>
            </>
          )}
          <dt>Region path</dt>
          <dd>
            <code>{opDetail.region_path.join(' / ') || '(top level)'}</code>
          </dd>
          <dt>Lines</dt>
          <dd>
            {opDetail.line_start}–{opDetail.line_end} ({opDetail.side})
          </dd>
        </dl>
      )}
    </aside>
  )
}
