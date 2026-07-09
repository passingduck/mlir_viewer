import { useEffect } from 'react'
import { useViewerStore } from './store'

export function useGlobalKeys() {
  const { setPaletteOpen, stepPass, closeInspector } = useViewerStore()
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const typing =
        target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')
      if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
        event.preventDefault()
        setPaletteOpen(true)
      } else if (typing) {
        return
      } else if (event.key === '[') void stepPass(-1)
      else if (event.key === ']') void stepPass(1)
      else if (event.key === '/') {
        event.preventDefault()
        setPaletteOpen(true)
      } else if (event.key === 'Escape') closeInspector()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [setPaletteOpen, stepPass, closeInspector])
}
