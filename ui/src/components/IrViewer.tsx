import { useEffect, useRef } from 'react'
import { EditorState, RangeSetBuilder } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  keymap,
  lineNumbers,
} from '@codemirror/view'
import type { FunctionDiff, IrPage, IrSide, SelectableOp } from '../api'
import { lineClasses, type LineClass } from '../diffDecorations'
import { mlirLanguage } from '../mlirLanguage'

interface IrViewerProps {
  before: IrPage | null
  after: IrPage | null
  diff?: FunctionDiff | null
  beforeOps?: SelectableOp[]
  afterOps?: SelectableOp[]
  onSelectOp?: (uid: string) => void
}

const lineDecoration: Record<LineClass, Decoration> = {
  added: Decoration.line({ attributes: { class: 'diff-added' } }),
  removed: Decoration.line({ attributes: { class: 'diff-removed' } }),
  modified: Decoration.line({ attributes: { class: 'diff-modified' } }),
}

const selectableDecoration = Decoration.line({ attributes: { class: 'selectable-op' } })

export function operationAtLine(operations: SelectableOp[], line: number): SelectableOp | null {
  return (
    operations
      .filter((operation) => operation.line_start <= line && line <= operation.line_end)
      .sort(
        (left, right) =>
          left.line_end - left.line_start - (right.line_end - right.line_start) ||
          right.op_idx - left.op_idx,
      )[0] ?? null
  )
}

function selectionExtension(operations: SelectableOp[], onSelectOp?: (uid: string) => void) {
  return [
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet

        constructor(view: EditorView) {
          const builder = new RangeSetBuilder<Decoration>()
          const lines = new Set<number>()
          for (const operation of operations) {
            for (let line = operation.line_start; line <= operation.line_end; line += 1) {
              if (line >= 1 && line <= view.state.doc.lines) lines.add(line)
            }
          }
          for (const line of [...lines].sort((left, right) => left - right)) {
            const position = view.state.doc.line(line).from
            builder.add(position, position, selectableDecoration)
          }
          this.decorations = builder.finish()
        }
      },
      { decorations: (plugin) => plugin.decorations },
    ),
    EditorView.domEventHandlers({
      mousedown(event, view) {
        let position = view.posAtCoords({ x: event.clientX, y: event.clientY })
        if (position === null && event.target instanceof Node) {
          try {
            position = view.posAtDOM(event.target)
          } catch {
            position = null
          }
        }
        if (position === null) return false
        const operation = operationAtLine(operations, view.state.doc.lineAt(position).number)
        if (!operation) return false
        onSelectOp?.(operation.uid)
        event.preventDefault()
        return true
      },
    }),
    keymap.of([
      {
        key: 'Enter',
        run(view) {
          const line = view.state.doc.lineAt(view.state.selection.main.head).number
          const operation = operationAtLine(operations, line)
          if (!operation) return false
          onSelectOp?.(operation.uid)
          return true
        },
      },
    ]),
  ]
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
  operations,
  onSelectOp,
}: {
  side: IrSide
  page: IrPage | null
  diff?: FunctionDiff | null
  operations: SelectableOp[]
  onSelectOp?: (uid: string) => void
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
          selectionExtension(operations, onSelectOp),
        ],
      }),
    })
    return () => view.destroy()
  }, [diff, onSelectOp, operations, page, side])

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

export function IrViewer({
  before,
  after,
  diff,
  beforeOps = [],
  afterOps = [],
  onSelectOp,
}: IrViewerProps) {
  return (
    <section className="editor-grid" aria-label="IR snapshots">
      <EditorPane
        side="before"
        page={before}
        diff={diff}
        operations={beforeOps}
        onSelectOp={onSelectOp}
      />
      <EditorPane
        side="after"
        page={after}
        diff={diff}
        operations={afterOps}
        onSelectOp={onSelectOp}
      />
    </section>
  )
}
