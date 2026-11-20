import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import { App } from './App'

type Item = Record<string, string | number | boolean | null | undefined>
type Data = Record<string, Item[]>

function fixture(): Data {
  return {
    branches: [{ id: 'b1', code: 'LAB', name: 'Laboratorio', status: 'ACTIVE' }],
    roles: [],
    users: [{ id: 'u1', username: 'admin', display_name: 'Administrador', status: 'ACTIVE' }],
    memberships: [],
    lockers: [],
    compartments: [],
    locks: [],
    cameras: [],
    tools: [
      { id: 't1', asset_code: 'TOOL-001', name: 'Taladro', status: 'AVAILABLE', rfid_tag: 'R1' },
      { id: 't2', asset_code: 'TOOL-002', name: 'Llave', status: 'AVAILABLE', rfid_tag: null },
      { id: 't3', asset_code: 'TOOL-003', name: 'Martillo', status: 'ON_LOAN', rfid_tag: null },
    ],
    placements: [],
    authorizations: [],
    loans: [],
    operations: [],
    audit: [{ id: 'a1', action: 'TOOL_PLACED', entity_type: 'Tool', entity_id: 't1', created_at: '2026-01-01T00:00:00Z' }],
  }
}

function jsonResponse(body: unknown, ok = true) {
  return Promise.resolve({ ok, status: ok ? 200 : 409, json: () => Promise.resolve(body) })
}

type Mutation = () => { ok: boolean; body: unknown }

/** Stub of the API: entity lists from `data`, deferred URLs answered by the
 * test (honoring AbortSignal like a real fetch). `capabilities` emulates the
 * Rust backend feature detection (absent → 404 like the Python backend). */
function stubApi(data: Data, options?: {
  deferred?: string[]
  mutation?: Mutation
  capabilities?: Record<string, boolean>
  presence?: Item[]
}) {
  const calls: { url: string; init?: RequestInit }[] = []
  const resolvers = new Map<string, (value: unknown) => void>()
  vi.stubGlobal(
    'fetch',
    vi.fn((input: string | URL, init?: RequestInit) => {
      const url = String(input)
      calls.push({ url, init })
      if (url === '/api/v1/auth/login')
        return jsonResponse({ access_token: 'lab-admin-token', token_type: 'bearer', user: data.users[0] })
      if (url === '/api/v1/dashboard')
        return jsonResponse({
          lockers: 1,
          tools_available: 1,
          tools_on_loan: 1,
          pending_loans: 0,
          recent_activity: [
            { id: 'a1', action: 'LOGIN', entity_type: 'User', entity_id: 'u1', created_at: '2026-01-01T00:00:00Z' },
          ],
        })
      if (url === '/api/v1/capabilities') {
        if (options?.capabilities) return jsonResponse(options.capabilities)
        return jsonResponse({ detail: 'Not Found' }, false)
      }
      if (url === '/api/v1/lockers/presence') return jsonResponse(options?.presence ?? [])
      if (options?.deferred?.includes(url)) {
        return new Promise((resolve, reject) => {
          resolvers.set(url, (value: unknown) => resolve(jsonResponse(value)))
          const signal = init?.signal
          if (!signal) return
          const onAbort = () => reject(new DOMException('aborted', 'AbortError'))
          if (signal.aborted) onAbort()
          else signal.addEventListener('abort', onAbort, { once: true })
        })
      }
      if (init?.method && init.method !== 'GET') {
        const mutation = options?.mutation?.() ?? { ok: true, body: {} }
        return jsonResponse(mutation.body, mutation.ok)
      }
      const entity = url.replace('/api/v1/', '').split('?')[0]
      return jsonResponse(data[entity] ?? [])
    }),
  )
  return { calls, resolvers }
}

async function enterPanel(data: Data, options?: {
  deferred?: string[]
  mutation?: Mutation
  capabilities?: Record<string, boolean>
  presence?: Item[]
}) {
  const api = stubApi(data, options)
  render(<App />)
  const form = screen.getByRole('button', { name: 'Ingresar al panel' }).closest('form')!
  fireEvent.submit(form)
  // 'TOOL_PLACED' viene de data.audit, que solo existe cuando `load()` terminó.
  await waitFor(() => expect(screen.getByText('TOOL_PLACED')).toBeTruthy())
  return api
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

test('el historial muestra solo la herramienta seleccionada y responde en orden invertido', async () => {
  const data = fixture()
  const api = await enterPanel(data, {
    deferred: ['/api/v1/tools/t1/history', '/api/v1/tools/t2/history'],
  })
  fireEvent.click(screen.getByRole('button', { name: /Inventario/ }))
  fireEvent.click(screen.getByText('TOOL-001'))
  await waitFor(() => expect(api.resolvers.get('/api/v1/tools/t1/history')).toBeTruthy())

  // Cambio de selección ANTES de que responda la primera herramienta.
  fireEvent.click(screen.getByText('TOOL-002'))
  await waitFor(() => expect(api.resolvers.get('/api/v1/tools/t2/history')).toBeTruthy())

  // La segunda responde primero y pinta su historial propio.
  api.resolvers.get('/api/v1/tools/t2/history')!([
    { id: 'p2', tool_id: 't2', locker_id: 'locker-B', compartment_id: 'c9', placed_at: '2026-11-01T10:00:00Z', removed_at: null },
  ])
  await waitFor(() => expect(screen.getByText('locker-B')).toBeTruthy())

  // La respuesta tardía de la selección anterior llega ahora: debe ignorarse.
  api.resolvers.get('/api/v1/tools/t1/history')!([
    { id: 'p1', tool_id: 't1', locker_id: 'locker-A', compartment_id: 'c1', placed_at: '2026-10-01T10:00:00Z', removed_at: '2026-10-02T10:00:00Z' },
  ])
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(screen.queryByText('locker-A')).toBeNull()
  expect(screen.getByText('locker-B')).toBeTruthy()
  // La ubicación activa es la de la herramienta visible.
  expect(screen.getByText(/Ubicación actual:/).textContent).toContain('c9')
})

test('el historial vacío muestra su estado explícito', async () => {
  const data = fixture()
  await enterPanel(data)
  fireEvent.click(screen.getByRole('button', { name: /Inventario/ }))
  fireEvent.click(screen.getByText('TOOL-001'))
  await waitFor(() => expect(screen.getByText('Sin movimientos registrados.')).toBeTruthy())
  expect(screen.queryByText('locker-A')).toBeNull()
})

test('editar estado envía el PATCH y oculta controles de estados de dominio', async () => {
  const data = fixture()
  const api = await enterPanel(data)
  fireEvent.click(screen.getByRole('button', { name: /Administración/ }))
  fireEvent.change(screen.getByLabelText('Entidad'), { target: { value: 'tools' } })

  // La herramienta disponible ofrece la lista administrativa de estados.
  const available = within(screen.getByText('TOOL-001').closest('tr')!)
  const select = available.getByLabelText('Estado') as HTMLSelectElement
  expect([...select.options].map((option) => option.value)).toEqual(['AVAILABLE', 'INACTIVE'])
  fireEvent.change(select, { target: { value: 'INACTIVE' } })

  const patch = api.calls.find(({ url, init }) => url === '/api/v1/tools/t1' && init?.method === 'PATCH')
  expect(patch).toBeTruthy()
  expect(JSON.parse(String(patch!.init!.body))).toEqual({ status: 'INACTIVE' })

  // ON_LOAN pertenece al dominio: sin selector ni desactivación manual.
  const onLoan = within(screen.getByText('TOOL-003').closest('tr')!)
  expect(onLoan.queryByLabelText('Estado')).toBeNull()
  expect(onLoan.queryByRole('button', { name: 'Desactivar' })).toBeNull()
})

test('los errores del backend se muestran al usuario', async () => {
  const data = fixture()
  await enterPanel(data, {
    mutation: () => ({ ok: false, body: { detail: 'La herramienta tiene un préstamo activo o pendiente' } }),
  })
  fireEvent.click(screen.getByRole('button', { name: /Administración/ }))
  fireEvent.change(screen.getByLabelText('Entidad'), { target: { value: 'tools' } })
  fireEvent.change(within(screen.getByText('TOOL-001').closest('tr')!).getByLabelText('Estado'), { target: { value: 'INACTIVE' } })
  await waitFor(() =>
    expect(screen.getByText('La herramienta tiene un préstamo activo o pendiente')).toBeTruthy(),
  )
})

// ---------------------------------------------------------------------------
// F4.1-A: observed presence and the capabilities gate.
// ---------------------------------------------------------------------------

const presenceRows = (over: Partial<Item> = {}): Item[] => [
  {
    locker_id: 'k1', code: 'LAB-LOCKER-001', name: 'Locker principal', administrative_status: 'ONLINE',
    presence: 'ONLINE', last_communication: '2026-01-01T00:00:00Z', age_seconds: 5,
    source: 'locker/LAB-LOCKER-001/status', ambiguous_code: false, updated_at: '2026-01-01T00:00:00Z',
    ...over, id: 'p1',
  },
  {
    locker_id: 'k2', code: 'LAB-LOCKER-002', name: 'Locker taller', administrative_status: 'ONLINE',
    presence: 'OFFLINE', last_communication: '2026-01-01T00:00:00Z', age_seconds: 5,
    source: 'locker/LAB-LOCKER-002/status', ambiguous_code: false, updated_at: '2026-01-01T00:00:00Z',
  },
]

test('la presencia observada se muestra solo cuando el backend la soporta', async () => {
  const data = fixture()
  data.lockers = [
    { id: 'k1', code: 'LAB-LOCKER-001', name: 'Locker principal', status: 'ONLINE' },
    { id: 'k2', code: 'LAB-LOCKER-002', name: 'Locker taller', status: 'ONLINE' },
  ]
  await enterPanel(data, { capabilities: { resolve: true, presence: true }, presence: presenceRows() })
  fireEvent.click(screen.getByRole('button', { name: /Lockers/ }))

  // Estado administrativo y presencia observada conviven por fila.
  const online = within(screen.getByText('LAB-LOCKER-001').closest('tr')!)
  expect(online.getAllByText('ONLINE').length).toBe(2)
  const offline = within(screen.getByText('LAB-LOCKER-002').closest('tr')!)
  expect(offline.getByText('OFFLINE')).toBeTruthy()

  // El detalle muestra la última comunicación observada.
  fireEvent.click(screen.getByText('LAB-LOCKER-002'))
  await waitFor(() => expect(screen.getByText(/Presencia observada/).textContent).toContain('OFFLINE'))
  expect(screen.getByText(/Presencia observada/).textContent).toContain('última comunicación')
})

test('el backend sin capacidad de presencia no recibe la solicitud', async () => {
  const data = fixture()
  data.lockers = [{ id: 'k1', code: 'LAB-LOCKER-001', name: 'Locker principal', status: 'ONLINE' }]
  const api = await enterPanel(data) // sin capabilities: como el backend Python
  fireEvent.click(screen.getByRole('button', { name: /Lockers/ }))
  expect(screen.queryByText('presence')).toBeNull()
  expect(screen.queryByText('last communication')).toBeNull()
  expect(api.calls.every(({ url }) => !url.includes('lockers/presence'))).toBe(true)
})

test('el dashboard no muestra todo listo mientras exista una intervención', async () => {
  const data = fixture()
  data.operations = [{
    id: 'op1', status: 'FAILED', command_type: 'RETURN', tool_id: 't1', loan_id: 'l1',
    correlation_id: 'c1', origin: 'rust', created_at: '2026-01-01T00:00:00Z',
    failure_reason: 'Compartment is occupied',
  }]
  data.loans = [{ id: 'l1', status: 'RETURN_PENDING', tool_id: 't1', user_id: 'u1' }]
  await enterPanel(data, { capabilities: { resolve: true, presence: true }, presence: presenceRows() })
  expect(screen.getByText('Atención requerida')).toBeTruthy()

  // Sin condiciones pendientes, el estado vuelve a estar listo.
  const clean = fixture()
  clean.lockers = [{ id: 'k1', code: 'LAB-LOCKER-001', name: 'Locker principal', status: 'ONLINE' }]
  await enterPanel(clean, { capabilities: { resolve: true, presence: true }, presence: presenceRows().slice(0, 1) })
  expect(screen.getByText('Todo listo para operar')).toBeTruthy()
})
