import { useEffect, useMemo } from 'react'
import type { PassNode } from '../api'

interface TimelineProps {
  roots: PassNode[]
  selectedPassId: number | null
  onSelect: (id: number) => void
}

interface PassRowsProps extends Omit<TimelineProps, 'roots'> {
  nodes: PassNode[]
  depth: number
}

function flatten(nodes: PassNode[], output: PassNode[] = []): PassNode[] {
  for (const node of nodes) {
    output.push(node)
    flatten(node.children, output)
  }
  return output
}

function PassRows({
  nodes,
  depth,
  selectedPassId,
  onSelect,
}: PassRowsProps) {
  return (
    <ul className="pass-list" role={depth === 0 ? 'tree' : 'group'}>
      {nodes.map((pass) => {
        const durationMs = (pass.end_ns - pass.start_ns) / 1_000_000
        return (
          <li key={pass.id} role="treeitem" aria-expanded={pass.children.length ? true : undefined}>
            <button
              className="pass-row"
              style={{ paddingInlineStart: `${12 + depth * 16}px` }}
              aria-current={selectedPassId === pass.id ? 'true' : undefined}
              onClick={() => onSelect(pass.id)}
            >
              <span className="pass-name">{pass.name}</span>
              <span className="pass-meta">
                <span>{durationMs.toFixed(2)} ms</span>
                <span className={pass.ir_changed ? 'changed' : 'no-change'}>
                  {pass.ir_changed ? 'changed' : 'no-op'}
                </span>
              </span>
            </button>
            {pass.children.length > 0 && (
              <PassRows
                nodes={pass.children}
                depth={depth + 1}
                selectedPassId={selectedPassId}
                onSelect={onSelect}
              />
            )}
          </li>
        )
      })}
    </ul>
  )
}

export function Timeline({ roots, selectedPassId, onSelect }: TimelineProps) {
  const ordered = useMemo(() => flatten(roots), [roots])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== '[' && event.key !== ']') return
      const current = ordered.findIndex((pass) => pass.id === selectedPassId)
      if (current < 0) return
      const next = event.key === ']' ? current + 1 : current - 1
      if (ordered[next]) {
        event.preventDefault()
        onSelect(ordered[next].id)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onSelect, ordered, selectedPassId])

  return <PassRows nodes={roots} depth={0} selectedPassId={selectedPassId} onSelect={onSelect} />
}
