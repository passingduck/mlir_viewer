import type { HistoryStep, OpHistory } from '../api'

interface HistoryViewProps {
  history: OpHistory | null
  onViewIr: (passId: number) => void
}

function mergeKey(step: HistoryStep): string | null {
  return step.after ? `${step.pass_id}:${step.after.op_idx}` : null
}

export function HistoryView({ history, onViewIr }: HistoryViewProps) {
  if (!history) return <div className="status">Loading operation history…</div>

  const mergeCounts = new Map<string, number>()
  for (const step of history.steps) {
    const key = mergeKey(step)
    if (key) mergeCounts.set(key, (mergeCounts.get(key) ?? 0) + 1)
  }
  const shownMerges = new Set<string>()

  return (
    <section className="history-view" aria-label="Operation history">
      <header className="history-header">
        <div>
          <span className="label">Operation history</span>
          <h1>
            {history.first_name} → {history.last_name}
          </h1>
        </div>
        <code>{history.uid}</code>
      </header>
      {history.steps.length === 0 ? (
        <div className="status">No transitions recorded for this operation.</div>
      ) : (
        <ol className="history-timeline">
          {history.steps.map((step) => {
            const key = mergeKey(step)
            const mergeCount = key ? (mergeCounts.get(key) ?? 0) : 0
            const showMerge = Boolean(key && mergeCount > 1 && !shownMerges.has(key))
            if (showMerge && key) shownMerges.add(key)
            const confidence = step.confidence.kind
            return (
              <li
                className={`history-step ${confidence}`}
                key={`${step.pass_id}-${step.before?.op_idx ?? 'none'}-${step.after?.op_idx ?? 'none'}-${step.evidence.map((item) => `${item.seq}:${item.source}:${item.pattern ?? ''}`).join('|')}`}
              >
                {showMerge && <div className="merge-label">{mergeCount} predecessors merge</div>}
                <div className="history-step-heading">
                  <strong>{step.pass_name}</strong>
                  <span className={`change-kind ${step.change}`}>{step.change}</span>
                  {step.confidence.kind === 'inferred' && (
                    <span className="confidence">{step.confidence.score}% inferred</span>
                  )}
                </div>
                <div className="history-transition">
                  <code>{step.before?.name ?? '∅'}</code>
                  <span aria-hidden="true">→</span>
                  <code>{step.after?.name ?? '∅'}</code>
                </div>
                <div className="history-evidence">
                  {step.evidence.map((evidence) => (
                    <span
                      className="evidence"
                      key={`${evidence.seq}-${evidence.source}-${evidence.pattern ?? ''}`}
                    >
                      <span className="chip">{evidence.source}</span>
                      {evidence.pattern && <code>{evidence.pattern}</code>}
                    </span>
                  ))}
                </div>
                <button type="button" className="view-ir" onClick={() => onViewIr(step.pass_id)}>
                  View IR
                </button>
              </li>
            )
          })}
        </ol>
      )}
    </section>
  )
}
