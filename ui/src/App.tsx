import { useEffect } from 'react'
import type { PassNode } from './api'
import { useViewerStore } from './store'
import './styles.css'

function PassList({ nodes, depth = 0 }: { nodes: PassNode[]; depth?: number }) {
  const selectedPassId = useViewerStore((state) => state.selectedPassId)
  const selectPass = useViewerStore((state) => state.selectPass)
  return (
    <ul className="pass-list">
      {nodes.map((pass) => (
        <li key={pass.id}>
          <button
            className="pass-row"
            style={{ paddingInlineStart: `${12 + depth * 16}px` }}
            aria-current={selectedPassId === pass.id ? 'true' : undefined}
            onClick={() => void selectPass(pass.id)}
          >
            <span>{pass.name}</span>
            <span className={pass.ir_changed ? 'changed' : 'no-change'}>
              {pass.ir_changed ? 'changed' : 'no-op'}
            </span>
          </button>
          {pass.children.length > 0 && <PassList nodes={pass.children} depth={depth + 1} />}
        </li>
      ))}
    </ul>
  )
}

export function App() {
  const { status, error, info, roots, before, after, load } = useViewerStore()
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
          <nav aria-label="Pass timeline"><PassList nodes={roots} /></nav>
          <section className="editor-grid" aria-label="IR snapshots">
            <article><h2>Before</h2><pre>{before?.text ?? 'No snapshot'}</pre></article>
            <article><h2>After</h2><pre>{after?.text ?? 'No snapshot'}</pre></article>
          </section>
        </main>
      )}
      {error && status !== 'error' && <div className="toast" role="alert">{error}</div>}
    </div>
  )
}
