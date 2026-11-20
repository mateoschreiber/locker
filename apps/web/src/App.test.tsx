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

/** Mirrors the backend intervention rule for `attention=1` listings. */
function needsAttention(operation: Item, data: Data) {
  const loan = (data.loans ?? []).find((candidate) => candidate.id === operation.loan_id)
  return operation.status === 'UNKNOWN'
    || (operation.status === 'FAILED' && ['CHECKOUT_PENDING', 'RETURN_PENDING'].includes(String(loan?.status)))
}

/** Stub of the API: entity lists from `data`, deferred URLs answered by the
 * test (honoring AbortSignal like a real fetch). `capabilities` emulates the
 * Rust backend feature detection (absent → 404 like the Python backend); with
 * `pagination` the query-string URLs answer the Rust envelope, applying the
 * same server-side filters the real backend does. */
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
      const [entity, query] = url.replace('/api/v1/', '').split('?')
      let items = data[entity] ?? []
      if (query && options?.capabilities?.pagination) {
        const params = new URLSearchParams(query)
        // Server-side contracts the real Rust backend applies.
        if (params.get('status')) items = items.filter((item) => item.status === params.get('status'))
        if (entity === 'operations' && params.get('attention') === '1')
          items = items.filter((operation) => needsAttention(operation, data))
        if (entity === 'operations' && options.capabilities.enriched_operations) {
          const byId = (resource: string, id: unknown) => (data[resource] ?? []).find((item) => item.id === id)
          items = items.map((operation) => {
            const loan = byId('loans', operation.loan_id)
            return {
              ...operation,
              tool_name: String(byId('tools', operation.tool_id)?.name ?? ''),
              locker_code: String(byId('lockers', operation.locker_id)?.code ?? ''),
              compartment_code: String(byId('compartments', operation.compartment_id)?.code ?? ''),
              user_display_name: String(byId('users', loan?.user_id)?.display_name ?? ''),
              loan_status: String(loan?.status ?? ''),
              requires_intervention: needsAttention(operation, data),
            }
          })
        }
        return jsonResponse({ items, total: items.length, limit: Number(params.get('limit')), offset: Number(params.get('offset') ?? 0) })
      }
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
  // La vista inicial pinta la actividad reciente que llega en /dashboard.
  await waitFor(() => expect(screen.getByText('LOGIN')).toBeTruthy())
  return api
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  // The hidden-tab test shadows document.visibilityState; restore the getter.
  delete (document as unknown as Record<string, unknown>).visibilityState
})

// ---------------------------------------------------------------------------
// F4.2-B: per-view loading — only the active view's dependencies are polled.
// ---------------------------------------------------------------------------

test('la vista activa consulta solo sus dependencias y cambia con ella', async () => {
  const data = fixture()
  const api = await enterPanel(data) // sin capabilities: como el backend Python
  const urls = new Set(api.calls.map(({ url }) => url))
  // inicio (modo Python): dashboard + fallback del hero; más las referencias.
  for (const present of ['/api/v1/dashboard', '/api/v1/operations', '/api/v1/loans', '/api/v1/branches', '/api/v1/roles', '/api/v1/users', '/api/v1/lockers', '/api/v1/compartments']) {
    expect(urls.has(present), present).toBe(true)
  }
  // Nada de lo que ninguna vista en pantalla necesita.
  for (const absent of ['/api/v1/tools', '/api/v1/placements', '/api/v1/audit', '/api/v1/memberships', '/api/v1/cameras']) {
    expect(urls.has(absent), absent).toBe(false)
  }

  // Cambio de vista: el sondeo pasa a consultar lo que esa vista necesita.
  fireEvent.click(screen.getByRole('button', { name: /Inventario/ }))
  await waitFor(() => expect(api.calls.some(({ url }) => url === '/api/v1/tools')).toBe(true))
  expect(api.calls.every(({ url }) => url !== '/api/v1/audit')).toBe(true)
})

test('una mutación refresca la vista activa y sus referencias, no el laboratorio entero', async () => {
  const data = fixture()
  const api = await enterPanel(data, { mutation: () => ({ ok: true, body: {} }) })
  fireEvent.click(screen.getByRole('button', { name: /Operación$/ }))
  await waitFor(() => expect(screen.getByRole('option', { name: /TOOL-001/ })).toBeTruthy())
  const authorizationsBefore = api.calls.filter(({ url }) => url === '/api/v1/authorizations').length
  const branchesBefore = api.calls.filter(({ url }) => url === '/api/v1/branches').length
  fireEvent.change(screen.getByLabelText('Herramienta'), { target: { value: 't1' } })
  fireEvent.change(screen.getByLabelText('Usuario'), { target: { value: 'u1' } })
  fireEvent.click(screen.getByText('Crear autorización'))
  await waitFor(() =>
    expect(api.calls.filter(({ url }) => url === '/api/v1/authorizations').length).toBeGreaterThan(authorizationsBefore))
  expect(api.calls.filter(({ url }) => url === '/api/v1/branches').length).toBeGreaterThan(branchesBefore)
  // Lo que ninguna vista en pantalla usa no se consulta por la mutación.
  expect(api.calls.every(({ url }) => url !== '/api/v1/audit')).toBe(true)
})

test('la primera carga muestra el estado de carga y luego el vacío explícito', async () => {
  const data = fixture()
  data.audit = []
  const api = await enterPanel(data, { deferred: ['/api/v1/audit'] })
  fireEvent.click(screen.getByRole('button', { name: /Actividad/ }))
  expect(screen.getByText('Cargando…')).toBeTruthy()
  api.resolvers.get('/api/v1/audit')!([])
  await waitFor(() => expect(within(screen.getByRole('region', { name: 'Auditoría' })).getByText('Sin registros.')).toBeTruthy())
})

test('un ciclo sin respuesta no se solapa con el siguiente', async () => {
  const data = fixture()
  const api = await enterPanel(data, { deferred: ['/api/v1/operations'] })
  // La vista inicio (modo Python) también consulta operations: se mide el
  // delta desde el cambio de vista, no el total de la sesión.
  const before = api.calls.filter(({ url }) => url === '/api/v1/operations').length
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  // El primer ciclo de la vista queda pendiente: los siguientes se descartan.
  await new Promise((resolve) => setTimeout(resolve, 9000))
  expect(api.calls.filter(({ url }) => url === '/api/v1/operations').length - before).toBe(1)
  api.resolvers.get('/api/v1/operations')!([])
  await waitFor(() => expect(within(screen.getByRole('region', { name: 'Operaciones' })).getByText('Sin registros.')).toBeTruthy())
}, 20000)

test('la pestaña oculta suspende el sondeo y al volver refresca de inmediato', async () => {
  const data = fixture()
  const api = await enterPanel(data)
  Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true })
  document.dispatchEvent(new Event('visibilitychange'))
  const paused = api.calls.length
  // Suficiente para que el intervalo de 4 s hubiera disparado si no se hubiera detenido.
  await new Promise((resolve) => setTimeout(resolve, 4500))
  expect(api.calls.length).toBe(paused)
  Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true })
  document.dispatchEvent(new Event('visibilitychange'))
  await waitFor(() => expect(api.calls.length).toBeGreaterThan(paused))
}, 15000)

// ---------------------------------------------------------------------------
// F4.2-A/B together: Rust mode consumes the paginated envelope, the
// server-side attention list and the enriched operations without polling
// the reference tables.
// ---------------------------------------------------------------------------

test('el modo Rust consulta el hero con la lista de atención del servidor', async () => {
  const data = fixture()
  data.operations = [{ id: 'op1', status: 'UNKNOWN', command_type: 'RETURN', tool_id: 't1', loan_id: 'l1', created_at: '2026-01-01T00:00:00Z' }]
  const api = await enterPanel(data, { capabilities: { resolve: true, presence: true, pagination: true, enriched_operations: true } })
  expect(screen.getByText('Atención requerida')).toBeTruthy()
  expect(api.calls.some(({ url }) => url === '/api/v1/operations?attention=1&limit=100')).toBe(true)
  // El hero ya no sondea operations ni loans completos contra Rust.
  expect(api.calls.every(({ url }) => url !== '/api/v1/operations')).toBe(true)
  expect(api.calls.every(({ url }) => url !== '/api/v1/loans')).toBe(true)
})

test('la búsqueda del inventario consulta al servidor en modo Rust', async () => {
  const data = fixture()
  const api = await enterPanel(data, { capabilities: { resolve: true, presence: true, pagination: true, enriched_operations: true } })
  fireEvent.click(screen.getByRole('button', { name: /Inventario/ }))
  await waitFor(() => expect(api.calls.some(({ url }) => url === '/api/v1/tools?status=AVAILABLE&limit=100')).toBe(true))
  // El filtro de estado es del servidor: el activo en préstamo no llega.
  await waitFor(() => expect(screen.getByText('TOOL-001')).toBeTruthy())
  expect(screen.queryByText('TOOL-003')).toBeNull()
  fireEvent.change(screen.getByLabelText('Buscar herramienta'), { target: { value: 'TAL' } })
  await waitFor(() => expect(api.calls.some(({ url }) => url.includes('/api/v1/tools?status=AVAILABLE&q=TAL&limit=100'))).toBe(true))
})

test('el panel de operaciones en modo Rust consume el enriquecimiento del servidor', async () => {
  const data = operationsFixture()
  const api = await enterPanel(data, { capabilities: { resolve: true, presence: true, pagination: true, enriched_operations: true } })
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  await waitFor(() => expect(screen.getByText('Taladro')).toBeTruthy())
  expect(api.calls.some(({ url }) => url === '/api/v1/operations?limit=100')).toBe(true)
  // Una sola consulta: ni tools ni loans para enriquecer las filas.
  expect(api.calls.every(({ url }) => url !== '/api/v1/tools')).toBe(true)
  expect(api.calls.every(({ url }) => url !== '/api/v1/loans')).toBe(true)
  // La regla de intervención llega del servidor (requires_intervention).
  const failed = within(screen.getByText('Llave').closest('tr')!)
  expect(failed.getByText('⚠')).toBeTruthy()
  const confirmed = within(screen.getByText('Taladro').closest('tr')!)
  expect(confirmed.queryByText('⚠')).toBeNull()
})

// ---------------------------------------------------------------------------
// Comportamiento conservado de las vistas (historial, administración,
// presencia, conciliación) bajo la nueva arquitectura de sondeo.
// ---------------------------------------------------------------------------

test('el historial muestra solo la herramienta seleccionada y responde en orden invertido', async () => {
  const data = fixture()
  const api = await enterPanel(data, {
    deferred: ['/api/v1/tools/t1/history', '/api/v1/tools/t2/history'],
  })
  fireEvent.click(screen.getByRole('button', { name: /Inventario/ }))
  await waitFor(() => expect(screen.getByText('TOOL-001')).toBeTruthy())
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
  await waitFor(() => expect(screen.getByText('TOOL-001')).toBeTruthy())
  fireEvent.click(screen.getByText('TOOL-001'))
  await waitFor(() => expect(screen.getByText('Sin movimientos registrados.')).toBeTruthy())
  expect(screen.queryByText('locker-A')).toBeNull()
})

test('editar estado envía el PATCH y oculta controles de estados de dominio', async () => {
  const data = fixture()
  const api = await enterPanel(data)
  fireEvent.click(screen.getByRole('button', { name: /Administración/ }))
  // La entidad por defecto es branches: la tabla muestra la sucursal LAB.
  await waitFor(() => expect(within(screen.getByRole('region', { name: 'Sucursales' })).getByText('LAB')).toBeTruthy())
  fireEvent.change(screen.getByLabelText('Entidad'), { target: { value: 'tools' } })
  await waitFor(() => expect(screen.getByText('TOOL-001')).toBeTruthy())

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
  await waitFor(() => expect(within(screen.getByRole('region', { name: 'Sucursales' })).getByText('LAB')).toBeTruthy())
  fireEvent.change(screen.getByLabelText('Entidad'), { target: { value: 'tools' } })
  await waitFor(() => expect(screen.getByText('TOOL-001')).toBeTruthy())
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
  await waitFor(() => expect(screen.getByText('LAB-LOCKER-001')).toBeTruthy())

  // Estado administrativo y presencia observada conviven por fila. Ambos
  // llegan por cadenas asíncronas independientes: se espera la condición
  // funcional, no un retardo.
  await waitFor(() => {
    const online = within(screen.getByText('LAB-LOCKER-001').closest('tr')!)
    expect(online.getAllByText('ONLINE').length).toBe(2)
    const offline = within(screen.getByText('LAB-LOCKER-002').closest('tr')!)
    expect(offline.getByText('OFFLINE')).toBeTruthy()
  })

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
  await waitFor(() => expect(screen.getByText('LAB-LOCKER-001')).toBeTruthy())
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
  await waitFor(() => expect(screen.getByText('Atención requerida')).toBeTruthy())

  // Sin condiciones pendientes, el estado vuelve a estar listo.
  const clean = fixture()
  clean.lockers = [{ id: 'k1', code: 'LAB-LOCKER-001', name: 'Locker principal', status: 'ONLINE' }]
  await enterPanel(clean, { capabilities: { resolve: true, presence: true }, presence: presenceRows().slice(0, 1) })
  expect(screen.getByText('Todo listo para operar')).toBeTruthy()
})

// ---------------------------------------------------------------------------
// F4.1-B: operations supervision and reconciliation (Python fallback mode).
// ---------------------------------------------------------------------------

function operationsFixture(): Data {
  const data = fixture()
  data.lockers = [{ id: 'k1', code: 'L1', name: 'Locker principal', status: 'ONLINE' }]
  data.compartments = [{ id: 'c1', code: 'C01', name: 'C01', locker_id: 'k1', status: 'ACTIVE', position: 1 }]
  data.loans = [
    { id: 'l1', status: 'RETURN_PENDING', tool_id: 't2', user_id: 'u1' },
    { id: 'l2', status: 'RETURNED', tool_id: 't1', user_id: 'u1', returned_at: '2026-01-02T00:00:00Z' },
    { id: 'l3', status: 'RETURN_PENDING', tool_id: 't3', user_id: 'u1' },
  ]
  data.operations = [
    { id: 'op1', status: 'CONFIRMED', command_type: 'CHECKOUT', tool_id: 't1', loan_id: 'l2', branch_id: 'b1', locker_id: 'k1', compartment_id: 'c1', correlation_id: 'corr-1', origin: 'rust', created_at: '2026-01-01T00:00:00Z', published_at: '2026-01-01T00:00:01Z', confirmed_at: '2026-01-01T00:00:02Z', failure_reason: null },
    { id: 'op2', status: 'FAILED', command_type: 'RETURN', tool_id: 't2', loan_id: 'l1', branch_id: 'b1', locker_id: 'k1', compartment_id: 'c1', correlation_id: 'corr-2', origin: 'rust', created_at: '2026-01-03T00:00:00Z', published_at: '2026-01-03T00:00:01Z', confirmed_at: '2026-01-03T00:00:05Z', failure_reason: 'Compartment is occupied' },
    { id: 'op3', status: 'UNKNOWN', command_type: 'RETURN', tool_id: 't3', loan_id: 'l3', branch_id: 'b1', locker_id: 'k1', compartment_id: 'c1', correlation_id: 'corr-3', origin: 'rust', created_at: '2026-01-04T00:00:00Z', published_at: '2026-01-04T00:00:01Z', confirmed_at: null, failure_reason: null },
  ]
  return data
}

test('el panel de operaciones enriquece, filtra y concilia con el veredicto real', async () => {
  const data = operationsFixture()
  const api = await enterPanel(data, {
    capabilities: { resolve: true, presence: true },
    mutation: () => ({ ok: true, body: { status: 'FAILED', failure_reason: 'Compartment is occupied' } }),
  })
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  await waitFor(() => expect(screen.getByText('Taladro')).toBeTruthy())

  // Nombres y códigos legibles; las que requieren atención van primero.
  expect(screen.queryByText(/^[0-9a-f]{8}-/)).toBeNull()
  const failed = within(screen.getByText('Llave').closest('tr')!)
  expect(failed.getByText('⚠')).toBeTruthy()
  const confirmed = within(screen.getByText('Taladro').closest('tr')!)
  expect(confirmed.queryByText('⚠')).toBeNull()

  // Filtro por estado.
  fireEvent.change(screen.getByLabelText('Filtro'), { target: { value: 'CONFIRMED' } })
  expect(screen.queryByText('Llave')).toBeNull()
  expect(screen.queryByText('Martillo')).toBeNull()
  expect(screen.getByText('Taladro')).toBeTruthy()
  fireEvent.change(screen.getByLabelText('Filtro'), { target: { value: 'ALL' } })

  // Detalle con la traza completa y formulario solo para la elegible.
  fireEvent.click(screen.getByText('Llave'))
  const detail = screen.getByText(/Correlación/).parentElement!
  expect(detail.textContent).toContain('corr-2')
  expect(detail.textContent).toContain('L1')
  expect(detail.textContent).toContain('C01')
  expect(detail.textContent).toContain('Administrador')
  expect(detail.textContent).toContain('Compartment is occupied')
  await waitFor(() => expect(screen.getByText('Conciliar operación…')).toBeTruthy())

  // El operador atestigua CONFIRMED; el backend responde con el estado
  // resultante REAL (FAILED por precondiciones) y así se informa.
  fireEvent.click(screen.getByText('Conciliar operación…'))
  fireEvent.change(screen.getByLabelText('Veredicto'), { target: { value: 'CONFIRMED' } })
  fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: 'verificado en sitio' } })
  fireEvent.change(screen.getByLabelText('Evidencia'), { target: { value: 'fotos y registro' } })
  fireEvent.click(screen.getByText('Confirmar resolución'))
  const resolveCall = api.calls.find(({ url }) => url === '/api/v1/operations/op2/resolve')
  expect(resolveCall).toBeTruthy()
  expect(resolveCall!.init!.method).toBe('POST')
  expect((resolveCall!.init!.headers as Record<string, string>).Authorization).toBe('Bearer lab-admin-token')
  expect(JSON.parse(String(resolveCall!.init!.body))).toEqual({
    outcome: 'CONFIRMED', reason: 'verificado en sitio', evidence: 'fotos y registro',
  })
  await waitFor(() => expect(screen.getByText(/Estado resultante: FAILED/).textContent).toContain('Compartment is occupied'))
})

test('solo las operaciones elegibles ofrecen conciliación', async () => {
  const data = operationsFixture()
  await enterPanel(data, { capabilities: { resolve: true, presence: true } })
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  await waitFor(() => expect(screen.getByText('Taladro')).toBeTruthy())

  // CONFIRMED: terminal, nada que conciliar.
  fireEvent.click(screen.getByText('Taladro'))
  await waitFor(() => expect(screen.getByText(/no es conciliable/)).toBeTruthy())
  expect(screen.queryByText('Conciliar operación…')).toBeNull()

  // UNKNOWN: elegible.
  fireEvent.click(screen.getByText('Martillo'))
  await waitFor(() => expect(screen.getByText('Conciliar operación…')).toBeTruthy())
})

test('sin capacidad de resolve el backend nunca recibe la solicitud', async () => {
  const data = operationsFixture()
  const api = await enterPanel(data) // sin capabilities: como el backend Python
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  await waitFor(() => expect(screen.getByText('Martillo')).toBeTruthy())
  fireEvent.click(screen.getByText('Martillo'))
  expect(screen.queryByText('Conciliar operación…')).toBeNull()
  expect(screen.getByText(/exclusiva del backend Rust/)).toBeTruthy()
  expect(api.calls.every(({ url }) => !url.includes('/resolve'))).toBe(true)
})

test('los rechazos del backend en la conciliación se muestran comprensibles', async () => {
  const data = operationsFixture()
  await enterPanel(data, {
    capabilities: { resolve: true, presence: true },
    mutation: () => ({ ok: false, body: { detail: 'La operación fallida ya no retiene recursos del dominio' } }),
  })
  fireEvent.click(screen.getByRole('button', { name: /Operaciones/ }))
  await waitFor(() => expect(screen.getByText('Llave')).toBeTruthy())
  fireEvent.click(screen.getByText('Llave'))
  await waitFor(() => expect(screen.getByText('Conciliar operación…')).toBeTruthy())
  fireEvent.click(screen.getByText('Conciliar operación…'))
  fireEvent.change(screen.getByLabelText('Motivo'), { target: { value: 'motivo' } })
  fireEvent.change(screen.getByLabelText('Evidencia'), { target: { value: 'evidencia' } })
  fireEvent.click(screen.getByText('Confirmar resolución'))
  await waitFor(() =>
    expect(screen.getByText('La operación fallida ya no retiene recursos del dominio')).toBeTruthy(),
  )
})
