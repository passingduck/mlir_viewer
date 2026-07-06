import * as elkModule from 'elkjs/lib/elk.bundled.js'
import type ElkType from 'elkjs/lib/elk.bundled.js'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'
import type { DataflowGraph } from '../api'
import type { Layout } from './render'

// elkjs ships CJS browserify bundles that break when the production build
// re-bundles them into a module worker ("c is not a constructor"). Follow the
// pattern elkjs documents for bundlers instead: keep elk-worker.min.js as an
// untouched static asset (?url) and let ELK spawn its own worker from it, so
// layout still runs off the main thread without re-bundling the worker code.
const elkExports = elkModule as { default?: unknown }
const ElkConstructor = (typeof elkExports.default === 'function'
  ? elkExports.default
  : elkModule) as unknown as typeof ElkType

// Constructing ELK spawns its worker immediately, so defer until the first
// layout request: jsdom (unit tests) has no Worker, and users who never open
// the graph view shouldn't pay for one.
let elk: InstanceType<typeof ElkType> | null = null

export async function layoutGraph(graph: DataflowGraph): Promise<Layout> {
  elk ??= new ElkConstructor({
    workerUrl: elkWorkerUrl,
    workerFactory: (url) => new Worker(url ?? elkWorkerUrl),
  })
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
  return { positions, edges }
}
