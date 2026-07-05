import { expect, test } from 'vitest'
import type { DataflowGraph } from '../api'
import { selectableUid } from './GraphView'

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
