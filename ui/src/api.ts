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

export const api = {
  traceInfo: () => getJson<TraceInfo>('/api/trace/info'),
  passes: () => getJson<PassNode[]>('/api/passes'),
  irPage: (passId: number, side: IrSide) =>
    getJson<IrPage>(`/api/passes/${passId}/ir?side=${side}&limit=262144`),
}
