import { useEffect, useRef, useState } from 'react'

export type Item = Record<string, string | number | boolean | null | undefined>

/** Thin fetch wrapper: JSON API errors surface their backend `detail`. */
export async function request(path: string, options?: RequestInit, signal?: AbortSignal) {
  const response = await fetch(`/api/v1${path}`, {
    ...options,
    headers: { 'Content-Type': 'application/json', ...(options?.headers as Record<string, string> | undefined) },
    signal,
  })
  if (!response.ok) throw new Error((await response.json().catch(() => ({ detail: 'Error de conexión' }))).detail)
  return response.json() as Promise<Item | Item[]>
}

/** Paginated envelopes answer `{items, ...}`; legacy endpoints bare arrays.
 * One normalization keeps both backend modes consuming the same lists. */
export function asItems(value: Item | Item[]): Item[] {
  if (Array.isArray(value)) return value
  return (value.items as Item[] | undefined) ?? []
}

/** Polls `fetcher` immediately and then every `intervalMs`, only while the
 * tab is visible; never overlaps a cycle; a change in `deps` aborts the
 * in-flight cycle so its stale answer is ignored and restarts the cadence.
 * The fetcher owns its error reporting; aborts are silent by design. */
export function usePolling(fetcher: (signal: AbortSignal) => Promise<void>, deps: unknown[], intervalMs = 4000) {
  // The latest closure, updated after each render (never during render, per
  // the refs rule) so cycles read fresh state without the fetcher being a
  // dependency itself; the deps below are what trigger reloads.
  const fetcherRef = useRef(fetcher)
  useEffect(() => { fetcherRef.current = fetcher })
  useEffect(() => {
    let timer = 0
    let controller: AbortController | null = null
    let inFlight = false
    const cycle = async () => {
      if (inFlight || !controller) return
      inFlight = true
      try { await fetcherRef.current(controller.signal) } catch { /* the fetcher reports its own errors */ } finally { inFlight = false }
    }
    const start = () => {
      controller = new AbortController()
      void cycle()
      timer = window.setInterval(() => void cycle(), intervalMs)
    }
    // Hidden tabs stop polling entirely; coming back refreshes at once.
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') {
        window.clearInterval(timer)
        controller?.abort()
      } else {
        start()
      }
    }
    if (document.visibilityState !== 'hidden') start()
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      document.removeEventListener('visibilitychange', onVisibility)
      window.clearInterval(timer)
      controller?.abort()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}

/** Debounced copy of `value` so typing refetches once, after the pause. */
export function useDebounced<T>(value: T, delayMs = 300): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delayMs)
    return () => window.clearTimeout(timer)
  }, [value, delayMs])
  return debounced
}
