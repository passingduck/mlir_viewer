import { expect, test } from 'vitest'
import type { DataflowGraph } from '../api'
import { nextSelectableNodeId, selectableUid } from './GraphView'

const graph: DataflowGraph = {
  nodes: [
    {
      id: 'op0',
      label: 'arith.addi',
      op_name: 'arith.addi',
      line_range: [1, 1],
      cluster: [],
      collapsed_count: 0,
      uid: 'op1.Zg.1.a.0',
    },
    {
      id: 'cluster0',
      label: '2 ops',
      op_name: '(cluster)',
      line_range: [0, 0],
      cluster: [0],
      collapsed_count: 2,
    },
  ],
  edges: [],
  clusters: [],
  truncated: false,
}

test('returns a UID only for selectable graph nodes', () => {
  expect(selectableUid(graph, 'op0')).toBe('op1.Zg.1.a.0')
  expect(selectableUid(graph, 'cluster0')).toBeNull()
  expect(selectableUid(graph, null)).toBeNull()
})

test('cycles keyboard selection through UID-bearing nodes only', () => {
  const withSecond = {
    ...graph,
    nodes: [
      ...graph.nodes,
      { ...graph.nodes[0], id: 'op1', uid: 'op1.Zg.1.a.1' },
    ],
  }
  expect(nextSelectableNodeId(withSecond, null, 1)).toBe('op0')
  expect(nextSelectableNodeId(withSecond, 'op0', 1)).toBe('op1')
  expect(nextSelectableNodeId(withSecond, 'op0', -1)).toBe('op1')
})
