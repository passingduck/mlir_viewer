import { useEffect, useRef, useState } from 'react'
import type { DataflowGraph } from '../api'
import { drawGraph, hitTest, type Layout, type ViewState } from '../graph/render'

interface GraphViewProps {
  graph: DataflowGraph | null
  diffEnabled: boolean
}

export function GraphView({ graph, diffEnabled }: GraphViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [layout, setLayout] = useState<Layout | null>(null)
  const [layingOut, setLayingOut] = useState(false)
  const viewRef = useRef<ViewState>({
    scale: 1,
    offsetX: 20,
    offsetY: 20,
    hoverId: null,
    selectedId: null,
  })

  useEffect(() => {
    if (!graph) {
      setLayout(null)
      return
    }
    setLayingOut(true)
    const worker = new Worker(new URL('../graph/layout.worker.ts', import.meta.url), {
      type: 'module',
    })
    worker.onmessage = (event: MessageEvent<Layout>) => {
      setLayout(event.data)
      setLayingOut(false)
      worker.terminate()
    }
    worker.postMessage({ graph })
    return () => worker.terminate()
  }, [graph])

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
      viewRef.current.selectedId = hitTest(layout, world.x, world.y)
      redraw()
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
      />
    </section>
  )
}
