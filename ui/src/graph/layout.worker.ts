import ELK from 'elkjs/lib/elk.bundled.js'
import type { DataflowGraph } from '../api'

const elk = new ELK()

self.onmessage = async (event: MessageEvent<{ graph: DataflowGraph }>) => {
  const { graph } = event.data
  const result = await elk.layout({
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'DOWN',
      'elk.spacing.nodeNode': '24',
      'elk.layered.spacing.nodeNodeBetweenLayers': '40',
    },
    children: graph.nodes.map((node) => ({ id: node.id, width: 160, height: 34 })),
    edges: graph.edges.map((edge, index) => ({
      id: `e${index}`,
      sources: [edge.from],
      targets: [edge.to],
    })),
  })
  const positions: Record<string, { x: number; y: number; width: number; height: number }> = {}
  for (const child of result.children ?? []) {
    positions[child.id] = {
      x: child.x ?? 0,
      y: child.y ?? 0,
      width: child.width ?? 160,
      height: child.height ?? 34,
    }
  }
  const edges = (result.edges ?? []).map((edge, index) => ({
    from: graph.edges[index].from,
    to: graph.edges[index].to,
    removed: graph.edges[index].removed,
    sections: (edge.sections ?? []).map((section) => ({
      startPoint: section.startPoint,
      endPoint: section.endPoint,
    })),
  }))
  ;(self as unknown as Worker).postMessage({ positions, edges })
}
