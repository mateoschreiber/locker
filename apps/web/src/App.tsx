import { FormEvent, type ReactNode, useEffect, useMemo, useState } from 'react'
import { asItems, type Item, request, useDebounced, usePolling } from './polling'
import './styles.css'

type Data = Record<string, Item[]>
type View = 'inicio' | 'operacion' | 'operaciones' | 'inventario' | 'lockers' | 'administracion' | 'actividad'

const entities = ['branches', 'roles', 'users', 'memberships', 'lockers', 'compartments', 'locks', 'cameras', 'tools', 'placements', 'authorizations', 'loans', 'operations', 'audit']
const labels: Record<string, string> = { branches: 'Sucursales', roles: 'Roles', users: 'Usuarios', memberships: 'Membresías', lockers: 'Lockers', compartments: 'Compartimientos', locks: 'Cerraduras', cameras: 'Cámaras', tools: 'Herramientas', placements: 'Ubicaciones', authorizations: 'Autorizaciones', loans: 'Préstamos', operations: 'Operaciones', audit: 'Auditoría' }
const nav: [View, string, string][] = [['inicio', '⌂', 'Inicio'], ['operacion', '↗', 'Operación'], ['operaciones', '⟳', 'Operaciones'], ['inventario', '▦', 'Inventario'], ['lockers', '▣', 'Lockers'], ['administracion', '⚙', 'Administración'], ['actividad', '◷', 'Actividad']]
// Administrative states the backend accepts per entity (#19); domain-only
// states (tools ON_LOAN) are deliberately absent and never offered.
const editableStatus: Record<string, string[]> = { branches: ['ACTIVE', 'INACTIVE'], users: ['ACTIVE', 'INACTIVE'], memberships: ['ACTIVE', 'INACTIVE'], lockers: ['ONLINE', 'INACTIVE'], compartments: ['ACTIVE', 'INACTIVE'], locks: ['LOCKED', 'INACTIVE'], cameras: ['SIMULATED', 'INACTIVE'], tools: ['AVAILABLE', 'INACTIVE'] }

// Reference entities: slow-changing lookups loaded on login and after every
// mutation, never on the 4 s cadence.
const REFS = ['branches', 'roles', 'users', 'lockers', 'compartments']

// Dynamic data each view really needs: only these keys are polled.
const VIEW_DEPS: Record<View, string[]> = {
  inicio: ['dashboard', 'operations', 'loans'],
  operacion: ['authorizations', 'loans', 'tools'],
  operaciones: ['operations'],
  inventario: ['tools'],
  lockers: ['lockers'],
  administracion: [],
  actividad: ['audit', 'authorizations', 'loans'],
}

const text = (value: unknown) => value === null || value === undefined ? '—' : String(value)
const date = (value: unknown) => value ? new Intl.DateTimeFormat('es-PY', { dateStyle: 'short', timeStyle: 'short' }).format(new Date(String(value))) : '—'

export function App() {
  const [loggedIn, setLoggedIn] = useState(false)
  const [view, setView] = useState<View>('inicio')
  const [data, setData] = useState<Data>({})
  const [loaded, setLoaded] = useState<Record<string, boolean>>({})
  const [dashboard, setDashboard] = useState<Item>({})
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<Item | null>(null)
  const [adminEntity, setAdminEntity] = useState('branches')
  // Backend feature detection (F4.1): Python answers 404 for /capabilities,
  // so pagination, enrichment, presence and reconciliation stay off against
  // the Python backend and the panel falls back to the legacy contract.
  const [caps, setCaps] = useState<Record<string, boolean>>({})
  const [capsReady, setCapsReady] = useState(false)
  const [presence, setPresence] = useState<Item[]>([])
  // Lab session token from the login response (never a hardcoded constant);
  // only the reconciliation request carries it.
  const [token, setToken] = useState('')
  const debouncedQuery = useDebounced(query)

  // Entity keys each view needs (the debounced query joins separately as a
  // poll trigger; it is not a fetch key).
  const viewKeys = useMemo(() => {
    if (view === 'inicio') return caps.pagination ? ['dashboard', 'attention'] : VIEW_DEPS.inicio
    if (view === 'operaciones') return caps.enriched_operations ? ['operations'] : ['operations', 'tools', 'loans']
    if (view === 'administracion') return [adminEntity]
    return VIEW_DEPS[view]
  }, [view, adminEntity, caps.pagination, caps.enriched_operations])
  const pollDeps = useMemo(() =>
    view === 'inventario' ? [...viewKeys, debouncedQuery] : viewKeys,
  [view, viewKeys, debouncedQuery])

  const pathFor = (key: string) => {
    if (key === 'dashboard') return '/dashboard'
    if (key === 'attention') return '/operations?attention=1&limit=100'
    if (key === 'tools') {
      if (!caps.pagination) return '/tools'
      // Inventory and the operation flow want the available stock; the
      // administration table must list every tool, including INACTIVE/ON_LOAN.
      if (view === 'inventario') return `/tools?status=AVAILABLE${debouncedQuery ? `&q=${encodeURIComponent(debouncedQuery)}` : ''}&limit=100`
      if (view === 'operacion') return '/tools?status=AVAILABLE&limit=100'
      return '/tools?limit=100'
    }
    if (key === 'operations') return caps.pagination ? '/operations?limit=100' : '/operations'
    if (key === 'audit') return caps.pagination ? '/audit?limit=100' : '/audit'
    if (key === 'authorizations') return caps.pagination ? '/authorizations?limit=100' : '/authorizations'
    if (key === 'loans') return caps.pagination ? '/loans?limit=100' : '/loans'
    return `/${key}`
  }

  const fetchEntities = async (keys: string[], signal?: AbortSignal) => {
    // Per-key commits: one slow endpoint never blocks the rest of the view.
    let failure: unknown = null
    let aborted = false
    await Promise.all(keys.map(async (key) => {
      try {
        const value = await request(pathFor(key), undefined, signal)
        if (key === 'dashboard') setDashboard(value as Item)
        else setData((previous) => ({ ...previous, [key]: asItems(value) }))
        setLoaded((previous) => ({ ...previous, [key]: true }))
      } catch (reason) {
        if (reason instanceof DOMException && reason.name === 'AbortError') { aborted = true; return }
        failure ??= reason
      }
    }))
    // Stale data survives a failed cycle (the old values stay on screen);
    // the banner is the desynchronization signal, like before.
    if (aborted) return
    if (failure) setError(failure instanceof Error ? failure.message : 'No se pudo cargar el laboratorio')
    else setError('')
  }

  usePolling(async (signal) => {
    if (!loggedIn || !capsReady) return
    await fetchEntities(viewKeys, signal)
  }, [loggedIn, capsReady, pollDeps])

  // Reference lookups (names/codes for UUID columns) load once per session.
  // Timeout-0 defers the fetch out of the effect body (React lint: no
  // cascading setState); the panel's original load() used the same shape.
  useEffect(() => {
    if (!loggedIn || !capsReady) return
    const timer = window.setTimeout(() => void fetchEntities(REFS), 0)
    return () => window.clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loggedIn, capsReady])

  useEffect(() => {
    if (!loggedIn) return
    void request('/capabilities').then((found) => setCaps(found as unknown as Record<string, boolean>)).catch(() => setCaps({})).finally(() => setCapsReady(true))
  }, [loggedIn])

  // Presence rides its own light poll (F4.1-A): only when the backend
  // advertises it and only for the views that show it.
  const presenceActive = view === 'inicio' || view === 'lockers'
  usePolling(async (signal) => {
    if (!loggedIn || !capsReady || !caps.presence || !presenceActive) return
    try { setPresence(asItems(await request('/lockers/presence', undefined, signal))) } catch (reason) { if (reason instanceof DOMException && reason.name === 'AbortError') return; setError(reason instanceof Error ? reason.message : 'No se pudo cargar la presencia') }
  }, [loggedIn, capsReady, caps.presence, presenceActive])

  const login = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); const form = new FormData(event.currentTarget)
    try {
      const session = await request('/auth/login', { method: 'POST', body: JSON.stringify({ username: form.get('username'), password: form.get('password') }) }) as Item
      setToken(String(session.access_token ?? ''))
      setLoggedIn(true)
    } catch { setError('Usuario o contraseña incorrectos') }
  }
  const post = async (path: string, body?: Item, method = 'POST') => {
    try {
      await request(path, { method, body: body ? JSON.stringify(body) : undefined })
      setNotice('Operación registrada correctamente.')
      // The mutation owns what it touched: refresh the active view plus the
      // reference lookups, never the whole laboratory.
      await fetchEntities([...new Set([...viewKeys, ...REFS])])
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'No se pudo completar la acción') }
  }
  // Reconciliation (F4.1-B): the backend response is the authority — the
  // resulting status (which may be FAILED even for a CONFIRMED verdict) is
  // what the operator sees.
  const resolve = async (operationId: string, outcome: string, reason: string, evidence: string) => {
    try {
      const result = await request(`/operations/${operationId}/resolve`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}` },
        body: JSON.stringify({ outcome, reason, evidence }),
      }) as Item
      setNotice(`Resolución registrada. Estado resultante: ${text(result.status)}${result.failure_reason ? ` — ${text(result.failure_reason)}` : ''}.`)
      await fetchEntities([...new Set([...viewKeys, ...REFS])])
    } catch (reason_) { setError(reason_ instanceof Error ? reason_.message : 'No se pudo completar la conciliación') }
  }

  // Readable names for technical UUIDs: the tables show code/name (UUID kept
  // reachable on hover); the reference lives in the polled or ref data.
  const lookup = (entity: string, id: unknown) => (data[entity] ?? []).find((item) => String(item.id) === String(id)) ?? null

  const activeTools = data.tools?.filter((tool) => tool.status === 'AVAILABLE') ?? []
  // With server-side search the backend already filtered; the local filter is
  // the Python-mode fallback (which also applies, harmlessly, to full lists).
  const tools = caps.pagination && view === 'inventario' ? data.tools ?? [] : activeTools.filter((tool) => `${tool.asset_code} ${tool.name}`.toLowerCase().includes(query.toLowerCase()))
  const interventions = caps.pagination
    ? data.attention ?? []
    : (data.operations ?? []).filter((op) => op.status === 'UNKNOWN' || (op.status === 'FAILED' && ['CHECKOUT_PENDING', 'RETURN_PENDING'].includes(text((data.loans ?? []).find((loan) => loan.id === op.loan_id)?.status))))
  // shortcut: the hero counts the attention page (limit=100), not the
  // envelope's total; upgrade to `total` if a lab ever exceeds 100 open
  // interventions (the "attention required" boolean stays correct either way).

  if (!loggedIn) return <main className="login-page"><section className="login-card"><p className="brand">LOCKER <span>LAB</span></p><h1>Control de herramientas</h1><p>Acceso administrativo del laboratorio.</p><form onSubmit={login}><label>Usuario<input name="username" defaultValue="admin" required /></label><label>Contraseña<input name="password" type="password" defaultValue="admin" required /></label><button>Ingresar al panel</button></form><small>Laboratorio: admin / admin</small>{error && <p className="message error">{error}<button onClick={() => setError('')}>×</button></p>}</section></main>

  return <div className="app-shell"><aside><div className="logo">LOCKER <b>LAB</b></div><nav>{nav.map(([key, icon, name]) => <button key={key} className={view === key ? 'selected' : ''} onClick={() => { setView(key); setSelected(null) }}><span>{icon}</span>{name}</button>)}</nav><div className="sidebar-footer"><span className="online-dot" />Laboratorio conectado</div></aside><main className="workspace"><header><div><p className="breadcrumb">Laboratorio / {nav.find((item) => item[0] === view)?.[2]}</p><h1>{nav.find((item) => item[0] === view)?.[2]}</h1></div><div className="user">admin <button onClick={() => setLoggedIn(false)}>Salir</button></div></header>{error && <p className="message error">{error}<button onClick={() => setError('')}>×</button></p>}{notice && <p className="message notice">{notice}<button onClick={() => setNotice('')}>×</button></p>}
    {view === 'inicio' && <Dashboard dashboard={dashboard} interventions={interventions} presence={presence} loading={!loaded.dashboard} />}
    {view === 'inventario' && <Inventory tools={tools} query={query} setQuery={setQuery} selected={selected} setSelected={setSelected} loading={!loaded.tools} />}
    {view === 'lockers' && <Lockers lockers={data.lockers ?? []} presence={presence} onSelect={setSelected} selected={selected} loading={!loaded.lockers} />}
    {view === 'operacion' && <Operations data={data} post={post} lookup={lookup} />}
    {view === 'operaciones' && <OperationsPanel data={data} caps={caps} resolve={resolve} loading={!loaded.operations} />}
    {view === 'administracion' && <Administration entity={adminEntity} setEntity={setAdminEntity} data={data} post={post} lookup={lookup} loading={!loaded[adminEntity]} />}
    {view === 'actividad' && <Activity audit={data.audit ?? []} authorizations={data.authorizations ?? []} loans={data.loans ?? []} auditLoading={!loaded.audit} />}
  </main></div>
}

function Dashboard({ dashboard, interventions, presence, loading }: { dashboard: Item; interventions: Item[]; presence: Item[]; loading: boolean }) {
  const cards = [['Lockers', dashboard.lockers], ['Herramientas disponibles', dashboard.tools_available], ['En préstamo', dashboard.tools_on_loan], ['Operaciones pendientes', dashboard.pending_loans]]
  const recent = (dashboard.recent_activity as unknown as Item[] | undefined) ?? []
  // Never claim "all set" while something needs an operator (F4.1-A): an
  // uncertain/failed operation holding a loan, or an administratively-online
  // locker without observed presence.
  const offline = presence.filter((row) => row.administrative_status === 'ONLINE' && row.presence !== 'ONLINE')
  const attention = interventions.length + offline.length
  return <><section className={attention ? 'hero warn' : 'hero'}><div><p>ESTADO DEL LABORATORIO</p><h2>{attention ? 'Atención requerida' : 'Todo listo para operar'}</h2><span>{attention ? `${interventions.length} operación(es) por conciliar, ${offline.length} locker(s) sin presencia confirmada.` : 'Los datos se actualizan automáticamente.'}</span></div><div className="hero-mark">{attention ? '!' : '✓'}</div></section><section className="cards">{cards.map(([label, value]) => <article key={String(label)}><small>{label}</small><strong>{text(value)}</strong></article>)}</section><section className="panel"><h2>Actividad reciente</h2><Table items={recent} columns={['action', 'entity_type', 'created_at']} label="Actividad reciente" /></section><section className="panel"><p className="legend">{loading ? 'Cargando datos del laboratorio…' : 'Sondeo por vista activa: solo esta vista consulta el backend.'}</p></section></>
}

function Inventory({ tools, query, setQuery, selected, setSelected, loading }: { tools: Item[]; query: string; setQuery: (v: string) => void; selected: Item | null; setSelected: (v: Item) => void; loading: boolean }) {
  // History of the tool on screen only; state lands in async callbacks so a
  // late answer from a previous selection can never overwrite it (#14).
  const [historyState, setHistoryState] = useState<{ id: string; items: Item[]; error: string }>({ id: '', items: [], error: '' })
  useEffect(() => {
    const id = selected?.id ? String(selected.id) : ''
    if (!id) return
    const controller = new AbortController()
    void request(`/tools/${id}/history`, undefined, controller.signal)
      .then((items) => setHistoryState({ id, items: asItems(items), error: '' }))
      .catch((reason: unknown) => {
        if (reason instanceof DOMException && reason.name === 'AbortError') return
        setHistoryState({ id, items: [], error: reason instanceof Error ? reason.message : 'No se pudo cargar el historial' })
      })
    return () => controller.abort()
  }, [selected?.id])
  const history = selected?.id && historyState.id === String(selected.id) ? historyState.items : []
  const historyError = selected?.id && historyState.id === String(selected.id) ? historyState.error : ''
  const active = history.find((placement) => !placement.removed_at)
  return <div className="split"><section className="panel"><div className="panel-title"><h2>Herramientas</h2><input placeholder="Buscar herramienta…" value={query} onChange={(event) => setQuery(event.target.value)} aria-label="Buscar herramienta" /></div><Table items={tools} columns={['asset_code', 'name', 'status']} onSelect={setSelected} label="Herramientas" loading={loading} /></section><section className="panel detail"><h2>Detalle de herramienta</h2>{historyError && <p className="message error">{historyError}<button onClick={() => setHistoryState({ id: '', items: [], error: '' })}>×</button></p>}{selected ? <><b>{text(selected.name)}</b><p>Código: {text(selected.asset_code)}</p><p>Estado: <Badge value={text(selected.status)} /></p><p>RFID: {text(selected.rfid_tag)}</p><p>Ubicación actual: {active ? text(active.compartment_id) : '—'}</p><h3>Historial de ubicaciones</h3>{history.length ? <Table items={history} columns={['locker_id', 'compartment_id', 'placed_at', 'removed_at']} label="Historial de ubicaciones" /> : <p>Sin movimientos registrados.</p>}</> : <p>Seleccione una herramienta para consultar su historial.</p>}</section></div>
}

function Lockers({ lockers, presence, selected, onSelect, loading }: { lockers: Item[]; presence: Item[]; selected: Item | null; onSelect: (item: Item) => void; loading: boolean }) {
  const [detailState, setDetailState] = useState<{ id: string; item: Item | null; error: string }>({ id: '', item: null, error: '' })
  useEffect(() => {
    const id = selected?.id ? String(selected.id) : ''
    if (!id) return
    const controller = new AbortController()
    void request(`/lockers/${id}/detail`, undefined, controller.signal)
      .then((item) => setDetailState({ id, item: item as Item, error: '' }))
      .catch((reason: unknown) => {
        if (reason instanceof DOMException && reason.name === 'AbortError') return
        setDetailState({ id, item: null, error: reason instanceof Error ? reason.message : 'No se pudo cargar el detalle del locker' })
      })
    return () => controller.abort()
  }, [selected?.id])
  const detail = selected?.id && detailState.id === String(selected.id) ? detailState.item : null
  const detailError = selected?.id && detailState.id === String(selected.id) ? detailState.error : ''
  const compartments = (detail?.compartments as unknown as Item[] | undefined) ?? []
  // Observed presence per locker; the columns only appear when the backend
  // provides the data, so the Python panel shows the plain list.
  const observed = new Map(presence.map((row) => [String(row.locker_id), row]))
  const rows = lockers.map((locker) => {
    const row = observed.get(String(locker.id))
    return row ? { ...locker, presence: row.presence, last_communication: row.last_communication, ambiguous_code: row.ambiguous_code } : locker
  })
  const presenceColumns = presence.length ? ['presence', 'last_communication'] : []
  const selectedPresence = selected ? observed.get(String(selected.id)) : undefined
  return <div className="split"><section className="panel"><h2>Lockers</h2><Table items={rows} columns={['code', 'name', 'status', ...presenceColumns]} onSelect={onSelect} label="Lockers" loading={loading} /></section><section className="panel"><h2>{selected ? `Locker ${text(selected.code)}` : 'Detalle del locker'}</h2>{detailError && <p className="message error">{detailError}<button onClick={() => setDetailState({ id: '', item: null, error: '' })}>×</button></p>}{selected ? <>{selectedPresence && <p className="presence-line">Presencia observada: <Badge value={text(selectedPresence.presence)} />{text(selectedPresence.ambiguous_code) === 'true' ? ' (código duplicado: no atribuible)' : ` — última comunicación ${date(selectedPresence.last_communication)}`}</p>}<div className="compartments">{compartments.map((item) => <article key={text(item.id)} className={item.tool ? 'occupied' : ''}><b>{text(item.code)}</b><span>{item.tool ? text((item.tool as unknown as Item).asset_code) : 'Libre'}</span><small>{text((item.lock as unknown as Item | undefined)?.status ?? 'Sin cerradura')}</small></article>)}</div></> : <p>Seleccione un locker para ver sus compartimientos.</p>}</section></div>
}

function Operations({ data, post, lookup }: { data: Data; post: (p: string, b?: Item) => Promise<void>; lookup: (entity: string, id: unknown) => Item | null }) { const [tool, setTool] = useState(''); const [user, setUser] = useState(''); const branch = data.branches?.[0]?.id ?? ''
  const nameOf = (entity: string, value: unknown, render: (item: Item) => string) => <span title={String(value)}>{lookup(entity, value) ? render(lookup(entity, value)!) : '—'}</span>
  const format = { tool_id: (value: unknown) => nameOf('tools', value, (item) => `${text(item.asset_code)} — ${text(item.name)}`), user_id: (value: unknown) => nameOf('users', value, (item) => text(item.display_name)) }
  const authActions = (item: Item) => <>
    {item.status === 'PENDING' && <button onClick={() => void post(`/authorizations/${item.id}/approve`)}>Aprobar</button>}
    {item.status === 'APPROVED' && <button onClick={() => void post(`/authorizations/${item.id}/checkout`)}>Retirar</button>}
    {(item.status === 'PENDING' || item.status === 'APPROVED') && <button className="quiet" onClick={() => void post(`/authorizations/${item.id}/cancel`)}>Cancelar</button>}
  </>
  const loanActions = (item: Item) => <>{item.status === 'ACTIVE' && <button onClick={() => void post(`/loans/${item.id}/return`)}>Devolver</button>}</>
  return <><div className="split"><section className="panel"><h2>Nueva autorización</h2><form onSubmit={(e) => { e.preventDefault(); void post('/authorizations', { tool_id: tool, user_id: user, branch_id: String(branch) }) }}><select value={tool} onChange={(e) => setTool(e.target.value)} aria-label="Herramienta" required><option value="">Herramienta</option>{data.tools?.filter((i) => i.status === 'AVAILABLE').map((i) => <option value={text(i.id)} key={text(i.id)}>{text(i.asset_code)} — {text(i.name)}</option>)}</select><select value={user} onChange={(e) => setUser(e.target.value)} aria-label="Usuario" required><option value="">Usuario</option>{data.users?.filter((i) => i.status === 'ACTIVE').map((i) => <option value={text(i.id)} key={text(i.id)}>{text(i.display_name)}</option>)}</select><button>Crear autorización</button></form></section><section className="panel"><h2>Flujo simulado</h2><p>El panel solicita abrir el compartimiento. El simulador confirma la acción por MQTT y actualiza el préstamo.</p><span className="legend">Actualización automática cada 4 segundos</span></section></div><section className="panel"><h2>Autorizaciones</h2><Table items={data.authorizations ?? []} columns={['status', 'tool_id', 'user_id', 'created_at']} format={format} actions={authActions} label="Autorizaciones" /></section><section className="panel"><h2>Préstamos</h2><Table items={data.loans ?? []} columns={['status', 'tool_id', 'user_id', 'checked_out_at']} format={format} actions={loanActions} label="Préstamos" /></section></> }
// ---------------------------------------------------------------------------
// F4.1-B: operations supervision and administrative reconciliation.
// ---------------------------------------------------------------------------

/** Client-side eligibility mirrors the backend rule (F4.0): an UNKNOWN, or a
 * FAILED that still holds a pending loan. The backend stays the authority:
 * a 409 from `resolve` is shown as-is. Only the Python-mode fallback uses
 * this; the Rust listing serves `requires_intervention` directly. */
const needsIntervention = (operation: Item, loans: Map<string, Item>) =>
  operation.status === 'UNKNOWN'
  || (operation.status === 'FAILED'
    && ['CHECKOUT_PENDING', 'RETURN_PENDING'].includes(text(loans.get(String(operation.loan_id))?.status)))

function OperationsPanel({ data, caps, resolve, loading }: { data: Data; caps: Record<string, boolean>; resolve: (operationId: string, outcome: string, reason: string, evidence: string) => Promise<void>; loading: boolean }) {
  const [statusFilter, setStatusFilter] = useState('ALL')
  const [selectedId, setSelectedId] = useState('')
  const enriched = Boolean(caps.enriched_operations)
  const byId = (resource: string) => new Map((data[resource] ?? []).map((item) => [String(item.id), item]))
  const loans = byId('loans')
  const rows = (data.operations ?? [])
    .filter((operation) => statusFilter === 'ALL' || operation.status === statusFilter)
    .map((operation): Item => {
      if (enriched) {
        // One JOIN answered the names (F4.2-A); no reference tables polled.
        return {
          ...operation,
          tool_name: text(operation.tool_name),
          locker_code: text(operation.locker_code),
          compartment_code: text(operation.compartment_code),
          user_name: text(operation.user_display_name),
          attention: operation.requires_intervention ? '⚠' : '',
        }
      }
      const tools = byId('tools'), lockers = byId('lockers'), compartments = byId('compartments'), users = byId('users')
      return {
        ...operation,
        tool_name: text(tools.get(String(operation.tool_id))?.name),
        locker_code: text(lockers.get(String(operation.locker_id))?.code),
        compartment_code: text(compartments.get(String(operation.compartment_id))?.code),
        user_name: text(users.get(String(loans.get(String(operation.loan_id))?.user_id))?.display_name),
        attention: needsIntervention(operation, loans) ? '⚠' : '',
      }
    })
    .sort((a, b) => Number(Boolean(b.attention)) - Number(Boolean(a.attention)))
  const selected = rows.find((row) => String(row.id) === selectedId) ?? null
  const selectedOperation = selected ? (data.operations ?? []).find((operation) => String(operation.id) === String(selected.id)) ?? selected : null
  const eligible = selectedOperation
    ? enriched
      ? selectedOperation.requires_intervention === true
      : needsIntervention(selectedOperation, loans)
    : false
  const statuses = ['ALL', 'PENDING', 'PUBLISHING', 'PUBLISHED', 'CONFIRMED', 'FAILED', 'UNKNOWN']
  return <div className="split">
    <section className="panel">
      <div className="panel-title"><h2>Operaciones</h2>
        <label className="status-select">Filtro<select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}>{statuses.map((status) => <option key={status} value={status}>{status === 'ALL' ? 'Todos' : status}</option>)}</select></label>
      </div>
      <Table items={rows} columns={['status', 'attention', 'command_type', 'tool_name', 'locker_code', 'compartment_code', 'user_name', 'created_at']} onSelect={(item) => setSelectedId(String(item.id))} label="Operaciones" loading={loading} />
    </section>
    <section className="panel detail">
      <h2>Detalle de operación</h2>
      {!selected ? <p>Seleccione una operación para ver su traza completa{caps.resolve ? ' y conciliarla si requiere intervención.' : '.'}</p> : <>
        <b>{text(selected.command_type)} — {text(selected.tool_name)}</b>
        <p>Estado: <Badge value={text(selected.status)} /> {selected.attention && <span className="badge attention">requiere intervención</span>}</p>
        <p>Locker {text(selected.locker_code)} · compartimento {text(selected.compartment_code)}</p>
        <p>Usuario: {text(selected.user_name)}</p>
        <p>Correlación: {text(selected.correlation_id)}</p>
        <p>Origen: {text(selected.origin)}</p>
        <p>Solicitada: {date(selected.created_at)} · publicada: {date(selected.published_at)} · confirmada: {date(selected.confirmed_at)}</p>
        <p>Motivo de fallo: {text(selected.failure_reason)}</p>
        <p className="legend">Identificadores técnicos: herramienta {text(selected.tool_id)}, préstamo {text(selected.loan_id)}, locker {text(selected.locker_id)}.</p>
        {caps.resolve && eligible
          ? <ResolveOperation operation={selected} resolve={resolve} />
          : <p className="legend">{caps.resolve ? 'Esta operación no es conciliable: el backend solo acepta UNKNOWN o FAILED que aún retenga un préstamo pendiente.' : 'El backend conectado no implementa la conciliación (es exclusiva del backend Rust).'}</p>}
      </>}
    </section>
  </div>
}

function ResolveOperation({ operation, resolve }: { operation: Item; resolve: (operationId: string, outcome: string, reason: string, evidence: string) => Promise<void> }) {
  const [outcome, setOutcome] = useState('FAILED')
  const [reason, setReason] = useState('')
  const [evidence, setEvidence] = useState('')
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    await resolve(String(operation.id), outcome, reason.trim(), evidence.trim())
    setBusy(false)
    setConfirming(false)
  }
  return <section className="panel resolve">
    <h3>Conciliación administrativa</h3>
    <p>La apertura solicitada quedó sin desenlace físico confirmado. La resolución es terminal y queda en auditoría con tu motivo y evidencia.</p>
    <p className="legend">CONFIRMED atestigua que la apertura ocurrió: se aplican los efectos de dominio y, si sus precondiciones fallan, la operación termina FAILED y el préstamo pendiente se libera. FAILED atestigua que no ocurrió: el préstamo se revierte y la reserva se libera. El backend es la autoridad final.</p>
    {!confirming
      ? <button onClick={() => setConfirming(true)}>Conciliar operación…</button>
      : <>
        <label>Veredicto<select value={outcome} onChange={(event) => setOutcome(event.target.value)}>
          <option value="FAILED">FAILED — la apertura no ocurrió</option>
          <option value="CONFIRMED">CONFIRMED — la apertura ocurrió</option>
        </select></label>
        <label>Motivo<textarea value={reason} onChange={(event) => setReason(event.target.value)} rows={2} /></label>
        <label>Evidencia<input value={evidence} onChange={(event) => setEvidence(event.target.value)} /></label>
        <div className="resolve-actions">
          <button disabled={busy || !reason.trim() || !evidence.trim()} onClick={() => void submit()}>Confirmar resolución</button>
          <button className="quiet" disabled={busy} onClick={() => setConfirming(false)}>Cancelar</button>
        </div>
      </>}
  </section>
}

function Administration({ entity, setEntity, data, post, lookup, loading }: { entity: string; setEntity: (v: string) => void; data: Data; post: (p: string, b?: Item, method?: string) => Promise<void>; lookup: (entity: string, id: unknown) => Item | null; loading: boolean }) { const [payload, setPayload] = useState('{\n  "code": "",\n  "name": ""\n}'); const editable = editableStatus[entity]
  // UUID columns render their readable reference (UUID kept on hover).
  const REF_COLUMNS: Record<string, [string, (item: Item) => string]> = { user_id: ['users', (u) => text(u.display_name)], branch_id: ['branches', (b) => text(b.name)], role_id: ['roles', (r) => text(r.name)], locker_id: ['lockers', (l) => text(l.code)], compartment_id: ['compartments', (c) => text(c.code)] }
  const format = Object.fromEntries(Object.entries(REF_COLUMNS).map(([column, [refEntity, render]]) => [column, (value: unknown) => <span title={String(value)}>{lookup(refEntity, value) ? render(lookup(refEntity, value)!) : '—'}</span>]))
  return <><section className="panel admin-toolbar"><label>Entidad<select value={entity} onChange={(e) => setEntity(e.target.value)}>{entities.filter((e) => !['authorizations', 'loans', 'operations', 'audit'].includes(e)).map((e) => <option key={e}>{e}</option>)}</select></label><p>Alta, edición y desactivación. Las referencias se muestran legibles; el UUID técnico queda visible al pasar el cursor.</p></section><div className="split"><section className="panel"><h2>{labels[entity]}</h2><Table items={data[entity] ?? []} columns={columnsFor(entity)} format={format} label={labels[entity]} loading={loading} actions={(item) => <>{editable?.includes(text(item.status)) && <label className="status-select">Estado<select value={text(item.status)} onChange={(e) => void post(`/${entity}/${item.id}`, { status: e.target.value }, 'PATCH')}>{editable.map((s) => <option key={s} value={s}>{s}</option>)}</select></label>}{editable && item.status !== 'INACTIVE' && editable.includes(text(item.status)) && <button className="quiet" onClick={() => void post(`/${entity}/${item.id}/deactivate`)}>Desactivar</button>}</>} /></section><section className="panel"><h2>Crear registro</h2><p>Complete el JSON según la entidad. Las relaciones usan el UUID de los listados (visible al pasar el cursor sobre la referencia).</p><textarea value={payload} onChange={(e) => setPayload(e.target.value)} rows={10} /><button onClick={() => { try { void post(`/${entity}`, JSON.parse(payload) as Item) } catch { alert('JSON inválido') } }}>Crear</button></section></div></> }
function Activity({ audit, authorizations, loans, auditLoading }: { audit: Item[]; authorizations: Item[]; loans: Item[]; auditLoading: boolean }) { return <div className="activity-grid"><section className="panel"><h2>Auditoría</h2><Table items={audit} columns={['action', 'entity_type', 'created_at']} label="Auditoría" loading={auditLoading} /></section><section className="panel"><h2>Autorizaciones</h2><Table items={authorizations} columns={['status', 'created_at']} label="Autorizaciones" /></section><section className="panel"><h2>Préstamos</h2><Table items={loans} columns={['status', 'checked_out_at', 'returned_at']} label="Préstamos" /></section></div> }
function columnsFor(entity: string) { return ({ branches: ['code', 'name', 'status'], roles: ['code', 'name'], users: ['username', 'display_name', 'status'], memberships: ['user_id', 'branch_id', 'role_id', 'status'], lockers: ['code', 'name', 'branch_id', 'status'], compartments: ['code', 'name', 'position', 'locker_id', 'status'], locks: ['compartment_id', 'hardware_address', 'status'], cameras: ['name', 'locker_id', 'status'], tools: ['asset_code', 'name', 'rfid_tag', 'status'] } as Record<string, string[]>)[entity] ?? ['id', 'status'] }
function Table({ items, columns, actions, onSelect, label, loading, format }: { items: Item[]; columns: string[]; actions?: (item: Item) => ReactNode; onSelect?: (item: Item) => void; label?: string; loading?: boolean; format?: Record<string, (value: unknown, item: Item) => ReactNode> }) { return <div className="table-wrap" role="region" aria-label={label} tabIndex={0}><table><thead><tr>{columns.map((c) => <th key={c}>{c.replaceAll('_', ' ')}</th>)}{actions && <th>Acciones</th>}</tr></thead><tbody>{loading && !items.length ? <tr><td colSpan={columns.length + 1}>Cargando…</td></tr> : items.length ? items.map((item) => <tr key={text(item.id)} onClick={() => onSelect?.(item)}>{columns.map((column) => <td key={column}>{format?.[column] ? format[column](item[column], item) : column.includes('_at') || column === 'last_communication' ? date(item[column]) : column === 'status' || column === 'presence' ? <Badge value={text(item[column])} /> : text(item[column])}</td>)}{actions && <td className="actions" onClick={(e) => e.stopPropagation()}>{actions(item)}</td>}</tr>) : <tr><td colSpan={columns.length + 1}>Sin registros.</td></tr>}</tbody></table></div> }
function Badge({ value }: { value: string }) { return <span className={`badge ${value.toLowerCase()}`}>{value.replaceAll('_', ' ')}</span> }
