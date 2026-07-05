import { decode } from '@msgpack/msgpack'

export interface TraceInfo {
  format_version: string
  pass_count: number
  meta: Record<string, string>
}

export interface PassNode {
  id: number
  name: string
  ir_before: number | null
  ir_after: number | null
  start_ns: number
  end_ns: number
  ir_changed: boolean
  children: PassNode[]
}

export type IrSide = 'before' | 'after'

export interface IrPage {
  pass_id: number
  side: IrSide
  text: string
  offset: number
  next_offset: number | null
  total_bytes: number
}

export interface FunctionInfo {
  name: string
  op_count: number
  has_before: boolean
  has_after: boolean
}

export type ChangeClass = 'added' | 'removed' | 'modified' | 'unchanged'

export interface OpChange {
  class: ChangeClass
  before: number | null
  after: number | null
  before_lines: [number, number] | null
  after_lines: [number, number] | null
  detail: string[]
}

export interface FunctionDiff {
  func: string
  changes: OpChange[]
}

export interface GraphNode {
  id: string
  label: string
  op_name: string
  line_range: [number, number]
  cluster: number[]
  change?: ChangeClass
  collapsed_count: number
  uid?: string
}

export interface GraphEdge {
  from: string
  to: string
  removed: boolean
}

export interface GraphCluster {
  path: number[]
  label: string
}

export interface DataflowGraph {
  nodes: GraphNode[]
  edges: GraphEdge[]
  clusters: GraphCluster[]
  truncated: boolean
}

export interface SelectableOp {
  uid: string
  op_idx: number
  name: string
  line_start: number
  line_end: number
}

export type SnapshotSide = 'before' | 'after'
export type HistoryChange = 'inserted' | 'erased' | 'replaced' | 'modified' | 'unchanged'
export type EvidenceSource = 'listener' | 'action' | 'fingerprint' | 'shared_snapshot'
export type LinkConfidence = { kind: 'exact' } | { kind: 'inferred'; score: number }

export interface HistoryEvidence {
  seq: number
  pattern: string | null
  source: EvidenceSource
}

export interface OpOccurrence {
  side: SnapshotSide
  op_idx: number
  name: string
  line_start: number
  line_end: number
  attr_summary: string
  location: string | null
}

export interface HistoryStep {
  pass_id: number
  pass_name: string
  change: HistoryChange
  before: OpOccurrence | null
  after: OpOccurrence | null
  evidence: HistoryEvidence[]
  confidence: LinkConfidence
}

export interface OpHistory {
  uid: string
  first_name: string
  last_name: string
  steps: HistoryStep[]
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message)
  }
}

async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(path)
  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const body = (await response.json()) as { error?: string }
      message = body.error ?? message
    } catch {
      // The status remains actionable when a proxy returns a non-JSON error.
    }
    throw new ApiError(response.status, message)
  }
  return response.json() as Promise<T>
}

async function getMsgpack<T>(path: string): Promise<T> {
  const response = await fetch(path)
  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const body = (await response.json()) as { error?: string }
      message = body.error ?? message
    } catch {
      // A non-JSON proxy error still has an actionable status.
    }
    throw new ApiError(response.status, message)
  }
  return decode(new Uint8Array(await response.arrayBuffer())) as T
}

export const api = {
  traceInfo: () => getJson<TraceInfo>('/api/trace/info'),
  passes: () => getJson<PassNode[]>('/api/passes'),
  irPage: (passId: number, side: IrSide) =>
    getJson<IrPage>(`/api/passes/${passId}/ir?side=${side}&limit=262144`),
  functions: (passId: number) => getJson<FunctionInfo[]>(`/api/passes/${passId}/functions`),
  diff: (passId: number, func: string) =>
    getMsgpack<FunctionDiff>(`/api/passes/${passId}/diff?func=${encodeURIComponent(func)}`),
  graph: (passId: number, func: string, diff: boolean, budget: number) =>
    getMsgpack<DataflowGraph>(
      `/api/graphs/dataflow?pass=${passId}&func=${encodeURIComponent(func)}&diff=${diff ? 1 : 0}&budget=${budget}`,
    ),
  selectableOps: (passId: number, side: IrSide, func: string) =>
    getMsgpack<SelectableOp[]>(
      `/api/passes/${passId}/ops?side=${side}&func=${encodeURIComponent(func)}`,
    ),
  opHistory: (uid: string) =>
    getMsgpack<OpHistory>(`/api/ops/${encodeURIComponent(uid)}/history`),
}
