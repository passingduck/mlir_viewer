import { Command } from 'cmdk'
import { useEffect, useState } from 'react'
import { api, type SearchResult } from '../api'
import { useViewerStore } from '../store'

export function CommandPalette() {
  const {
    paletteOpen,
    setPaletteOpen,
    roots,
    functions,
    selectedPassId,
    selectPass,
    selectFunc,
    toggleDiff,
    setViewMode,
    jumpToSearchResult,
  } = useViewerStore()
  const [query, setQuery] = useState('')
  const [ops, setOps] = useState<SearchResult[]>([])

  useEffect(() => {
    if (!paletteOpen || query.trim().length < 2 || selectedPassId === null) {
      setOps([])
      return
    }
    const handle = setTimeout(() => {
      api
        .searchOps(query, selectedPassId, 'pipeline')
        .then(setOps)
        .catch(() => setOps([]))
    }, 150)
    return () => clearTimeout(handle)
  }, [paletteOpen, query, selectedPassId])

  const leaves: { id: number; name: string }[] = []
  const walk = (nodes: typeof roots) => {
    for (const node of nodes) {
      if (node.children.length === 0) leaves.push({ id: node.id, name: node.name })
      else walk(node.children)
    }
  }
  walk(roots)
  const close = () => setPaletteOpen(false)

  return (
    <Command.Dialog open={paletteOpen} onOpenChange={setPaletteOpen} label="Command palette">
      <Command.Input value={query} onValueChange={setQuery} placeholder="Search passes, functions, ops…" />
      <Command.List>
        <Command.Empty>No results.</Command.Empty>
        <Command.Group heading="Passes">
          {leaves.map((leaf) => (
            <Command.Item key={leaf.id} onSelect={() => { void selectPass(leaf.id); close() }}>
              {leaf.name}
            </Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Functions">
          {functions.map((func) => (
            <Command.Item key={func.name} onSelect={() => { selectFunc(func.name); close() }}>
              {func.name}
            </Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Actions">
          <Command.Item onSelect={() => { setViewMode('text'); close() }}>View: Text</Command.Item>
          <Command.Item onSelect={() => { setViewMode('graph'); close() }}>View: Graph</Command.Item>
          <Command.Item onSelect={() => { toggleDiff(); close() }}>Toggle diff</Command.Item>
        </Command.Group>
        {ops.length > 0 && (
          <Command.Group heading="Operations">
            {ops.map((result) => (
              <Command.Item
                key={`${result.pass_id}-${result.side}-${result.op_idx}`}
                onSelect={() => { void jumpToSearchResult(result); close() }}
              >
                {result.excerpt} — {result.func}, pass {result.pass_id}:{result.line_start}
              </Command.Item>
            ))}
          </Command.Group>
        )}
      </Command.List>
    </Command.Dialog>
  )
}
