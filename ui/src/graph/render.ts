import type { ChangeClass, DataflowGraph } from '../api'

export interface NodeBox {
  x: number
  y: number
  width: number
  height: number
}

export interface LaidOutEdge {
  from: string
  to: string
  removed: boolean
  sections: {
    startPoint: { x: number; y: number }
    endPoint: { x: number; y: number }
  }[]
}

export interface Layout {
  positions: Record<string, NodeBox>
  edges: LaidOutEdge[]
}

export interface ViewState {
  scale: number
  offsetX: number
  offsetY: number
  hoverId: string | null
  selectedId: string | null
}

export function nodeColor(change?: ChangeClass): string {
  switch (change) {
    case 'added':
      return '#2ea043'
    case 'removed':
      return '#f85149'
    case 'modified':
      return '#d2a83c'
    default:
      return '#3a4658'
  }
}

export function hitTest(layout: Layout, worldX: number, worldY: number): string | null {
  for (const [id, box] of Object.entries(layout.positions)) {
    if (
      worldX >= box.x &&
      worldX <= box.x + box.width &&
      worldY >= box.y &&
      worldY <= box.y + box.height
    ) {
      return id
    }
  }
  return null
}

export function drawGraph(
  context: CanvasRenderingContext2D,
  layout: Layout,
  graph: DataflowGraph,
  view: ViewState,
): void {
  const { scale, offsetX, offsetY } = view
  context.save()
  context.setTransform(scale, 0, 0, scale, offsetX, offsetY)
  context.clearRect(
    -offsetX / scale,
    -offsetY / scale,
    context.canvas.width / scale,
    context.canvas.height / scale,
  )

  for (const edge of layout.edges) {
    context.strokeStyle = edge.removed ? 'rgba(248,81,73,0.7)' : '#4a5568'
    context.lineWidth = 1 / scale
    context.setLineDash(edge.removed ? [4 / scale, 3 / scale] : [])
    for (const section of edge.sections) {
      context.beginPath()
      context.moveTo(section.startPoint.x, section.startPoint.y)
      context.lineTo(section.endPoint.x, section.endPoint.y)
      context.stroke()
    }
  }
  context.setLineDash([])

  const nodesById = new Map(graph.nodes.map((node) => [node.id, node]))
  const showLabels = scale >= 0.5
  for (const [id, box] of Object.entries(layout.positions)) {
    const node = nodesById.get(id)
    context.globalAlpha = node?.change === 'removed' ? 0.5 : 1
    context.fillStyle = nodeColor(node?.change)
    context.fillRect(box.x, box.y, box.width, box.height)
    if (view.hoverId === id || view.selectedId === id) {
      context.strokeStyle = '#cdd6e3'
      context.lineWidth = 2 / scale
      context.strokeRect(box.x, box.y, box.width, box.height)
    }
    if (showLabels && node) {
      context.globalAlpha = 1
      context.fillStyle = '#0b0d12'
      context.font = '12px ui-monospace, monospace'
      const label = node.collapsed_count > 0 ? `${node.collapsed_count} ops` : node.label
      context.fillText(label.slice(0, 28), box.x + 6, box.y + box.height / 2 + 4)
    }
  }
  context.globalAlpha = 1
  context.restore()
}
