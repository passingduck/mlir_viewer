import { useEffect, useRef } from 'react'
import { EditorState, RangeSetBuilder } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  lineNumbers,
} from '@codemirror/view'
import type { FunctionDiff, IrPage, IrSide } from '../api'
import { lineClasses, type LineClass } from '../diffDecorations'
import { mlirLanguage } from '../mlirLanguage'

interface IrViewerProps {
  before: IrPage | null
  after: IrPage | null
  diff?: FunctionDiff | null
}

const lineDecoration: Record<LineClass, Decoration> = {
  added: Decoration.line({ attributes: { class: 'diff-added' } }),
  removed: Decoration.line({ attributes: { class: 'diff-removed' } }),
  modified: Decoration.line({ attributes: { class: 'diff-modified' } }),
}

function diffExtension(diff: FunctionDiff | null | undefined, side: IrSide) {
  const classes = diff ? lineClasses(diff, side, 1) : new Map<number, LineClass>()
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet

      constructor(view: EditorView) {
        const builder = new RangeSetBuilder<Decoration>()
        for (let line = 1; line <= view.state.doc.lines; line += 1) {
          const lineClass = classes.get(line)
          if (lineClass) {
            const position = view.state.doc.line(line).from
            builder.add(position, position, lineDecoration[lineClass])
          }
        }
        this.decorations = builder.finish()
      }
    },
    { decorations: (plugin) => plugin.decorations },
  )
}

const editorTheme = EditorView.theme({
  '&': { height: '100%', backgroundColor: '#0b0d12', color: '#cdd6e3' },
  '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' },
  '.cm-gutters': { backgroundColor: '#0e1117', color: '#556070', border: 'none' },
  '.cm-content': { caretColor: 'transparent' },
})

function EditorPane({
  side,
  page,
  diff,
}: {
  side: IrSide
  page: IrPage | null
  diff?: FunctionDiff | null
}) {
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
          diffExtension(diff, side),
        ],
      }),
    })
    return () => view.destroy()
  }, [diff, page, side])

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

export function IrViewer({ before, after, diff }: IrViewerProps) {
  return (
    <section className="editor-grid" aria-label="IR snapshots">
      <EditorPane side="before" page={before} diff={diff} />
      <EditorPane side="after" page={after} diff={diff} />
    </section>
  )
}
