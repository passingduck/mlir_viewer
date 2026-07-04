import { useEffect, useRef } from 'react'
import { EditorState } from '@codemirror/state'
import { EditorView, lineNumbers } from '@codemirror/view'
import type { IrPage, IrSide } from '../api'
import { mlirLanguage } from '../mlirLanguage'

interface IrViewerProps {
  before: IrPage | null
  after: IrPage | null
}

const editorTheme = EditorView.theme({
  '&': { height: '100%', backgroundColor: '#0b0d12', color: '#cdd6e3' },
  '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' },
  '.cm-gutters': { backgroundColor: '#0e1117', color: '#556070', border: 'none' },
  '.cm-content': { caretColor: 'transparent' },
})

function EditorPane({ side, page }: { side: IrSide; page: IrPage | null }) {
  const host = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!host.current || !page) return
    const view = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: page.text,
        extensions: [
          lineNumbers(),
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          mlirLanguage,
          editorTheme,
        ],
      }),
    })
    return () => view.destroy()
  }, [page])

  return (
    <article className="editor-pane">
      <h2>{side === 'before' ? 'Before' : 'After'}</h2>
      {page ? (
        <>
          <div className="editor-host" ref={host} />
          {page.next_offset !== null && <div className="truncated">First 256 KiB shown</div>}
        </>
      ) : (
        <div className="empty-snapshot">No {side} snapshot</div>
      )}
    </article>
  )
}

export function IrViewer({ before, after }: IrViewerProps) {
  return (
    <section className="editor-grid" aria-label="IR snapshots">
      <EditorPane side="before" page={before} />
      <EditorPane side="after" page={after} />
    </section>
  )
}
