import { encode } from '@msgpack/msgpack'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { api, type DataflowGraph, type FunctionDiff } from './api'

afterEach(() => vi.restoreAllMocks())

function mockFetch(body: Uint8Array | object, ok = true, contentType = 'application/msgpack') {
  const isBinary = body instanceof Uint8Array
  const binary = isBinary ? (body as Uint8Array).slice().buffer : new ArrayBuffer(0)
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok,
      status: ok ? 200 : 500,
      headers: { get: () => contentType },
      arrayBuffer: async () => binary,
      json: async () => body,
    })) as unknown as typeof fetch,
  )
}

describe('api msgpack decoding', () => {
  it('decodes a diff payload', async () => {
    const payload: FunctionDiff = { func: 'forward', changes: [] }
    mockFetch(encode(payload))

    const diff = await api.diff(3, 'forward')
    expect(diff.func).toBe('forward')
  })

  it('decodes a graph payload', async () => {
    const payload: DataflowGraph = { nodes: [], edges: [], clusters: [], truncated: false }
    mockFetch(encode(payload))

    const graph = await api.graph(3, 'forward', false, 2000)
    expect(graph.truncated).toBe(false)
  })
})
