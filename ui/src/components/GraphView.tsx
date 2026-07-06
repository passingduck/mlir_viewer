import { useEffect, useRef, useState } from 'react'
import type { DataflowGraph } from '../api'
import { layoutGraph } from '../graph/elkLayout'
import { drawGraph, hitTest, type Layout, type ViewState } from '../graph/render'

interface GraphViewProps {
  graph: DataflowGraph | null
  diffEnabled: boolean
  onSelectOp?: (uid: string) => void
}

export function selectableUid(graph: DataflowGraph | null, nodeId: string | null): string | null {
  if (!graph || nodeId === null) return null
  return graph.nodes.find((node) => node.id === nodeId)?.uid ?? null
}

export function nextSelectableNodeId(
  graph: DataflowGraph | null,
  currentId: string | null,
  direction: 1 | -1,
): string | null {
  const ids = graph?.nodes.filter((node) => node.uid).map((node) => node.id) ?? []
  if (ids.length === 0) return null
  const current = ids.indexOf(currentId ?? '')
  if (current === -1) return direction === 1 ? ids[0] : ids[ids.length - 1]
  return ids[(current + direction + ids.length) % ids.length]
}

export function GraphView({ graph, diffEnabled, onSelectOp }: GraphViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const graphKey = graph
    ? `${graph.nodes.map((node) => node.id).join(',')}|${graph.edges.map((edge) => `${edge.from}>${edge.to}`).join(',')}`
    : ''
  const [layoutState, setLayoutState] = useState<{ key: string; layout: Layout } | null>(null)
  const layout = layoutState?.key === graphKey ? layoutState.layout : null
  // GraphView is only mounted in graph mode, so a null graph means the fetch
  // is still in flight; the busy status must cover that window too, or
  // keyboard selection silently no-ops against an empty graph.
  const layingOut = layout === null
  const viewRef = useRef<ViewState>({
    scale: 1,
    offsetX: 20,
    offsetY: 20,
    hoverId: null,
    selectedId: null,
  })

  useEffect(() => {
    if (!graph) {
      setLayoutState(null)
      return
    }
    let cancelled = false
    layoutGraph(graph)
      .then((result) => {
        if (!cancelled) setLayoutState({ key: graphKey, layout: result })
      })
      .catch((error: unknown) => {
        if (!cancelled) console.error('graph layout failed', error)
      })
    return () => {
      cancelled = true
    }
  }, [graph, graphKey])

  const redraw = () => {
    const canvas = canvasRef.current
    if (!canvas || !layout || !graph) return
    const context = canvas.getContext('2d')
    if (context) drawGraph(context, layout, graph, viewRef.current)
  }

  useEffect(redraw, [graph, layout])

  const toWorld = (clientX: number, clientY: number) => {
    const canvas = canvasRef.current!
    const bounds = canvas.getBoundingClientRect()
    const view = viewRef.current
    return {
      x: (clientX - bounds.left - view.offsetX) / view.scale,
      y: (clientY - bounds.top - view.offsetY) / view.scale,
    }
  }

  const onWheel = (event: React.WheelEvent) => {
    event.preventDefault()
    const view = viewRef.current
    const factor = event.deltaY < 0 ? 1.1 : 0.9
    view.scale = Math.min(4, Math.max(0.15, view.scale * factor))
    redraw()
  }

  const dragging = useRef<{ x: number; y: number; moved: boolean } | null>(null)
  const onPointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId)
    dragging.current = { x: event.clientX, y: event.clientY, moved: false }
  }
  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const view = viewRef.current
    if (dragging.current) {
      const dx = event.clientX - dragging.current.x
      const dy = event.clientY - dragging.current.y
      dragging.current = {
        x: event.clientX,
        y: event.clientY,
        moved: dragging.current.moved || Math.abs(dx) + Math.abs(dy) > 3,
      }
      view.offsetX += dx
      view.offsetY += dy
      redraw()
      return
    }
    if (layout) {
      const world = toWorld(event.clientX, event.clientY)
      const hovered = hitTest(layout, world.x, world.y)
      if (hovered !== view.hoverId) {
        view.hoverId = hovered
        redraw()
      }
    }
  }
  const onPointerUp = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const moved = dragging.current?.moved ?? false
    dragging.current = null
    event.currentTarget.releasePointerCapture(event.pointerId)
    if (!moved && layout) {
      const world = toWorld(event.clientX, event.clientY)
      const selectedId = hitTest(layout, world.x, world.y)
      viewRef.current.selectedId = selectedId
      redraw()
      const uid = selectableUid(graph, selectedId)
      if (uid) onSelectOp?.(uid)
    }
  }
  const onKeyDown = (event: React.KeyboardEvent<HTMLCanvasElement>) => {
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
      viewRef.current.selectedId = nextSelectableNodeId(graph, viewRef.current.selectedId, 1)
      event.preventDefault()
      redraw()
    } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
      viewRef.current.selectedId = nextSelectableNodeId(graph, viewRef.current.selectedId, -1)
      event.preventDefault()
      redraw()
    } else if (event.key === 'Enter' || event.key === ' ') {
      const uid = selectableUid(graph, viewRef.current.selectedId)
      if (uid) {
        event.preventDefault()
        onSelectOp?.(uid)
      }
    }
  }

  return (
    <section className="graph-view" aria-label="Dataflow graph">
      <div className="graph-legend">
        {diffEnabled && (
          <>
            <span className="chip added">added</span>
            <span className="chip removed">removed</span>
            <span className="chip modified">modified</span>
          </>
        )}
        {graph?.truncated && <span className="chip warn">Graph truncated to node budget</span>}
        {graph?.clusters.length ? (
          <span className="chip">{graph.clusters.length} clusters collapsed</span>
        ) : null}
      </div>
      {layingOut && <div className="status">Laying out graph…</div>}
      <canvas
        ref={canvasRef}
        width={1200}
        height={800}
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onKeyDown={onKeyDown}
        tabIndex={0}
        aria-label="Dataflow graph; use arrow keys to select an operation and Enter to open history"
      />
    </section>
  )
}
