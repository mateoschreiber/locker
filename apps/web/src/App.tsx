import { FormEvent, type ReactNode, useEffect, useState } from 'react'
import './styles.css'

type Item = Record<string, string | number | boolean | null | undefined>
type Data = Record<string, Item[]>
type View = 'inicio' | 'operacion' | 'operaciones' | 'inventario' | 'lockers' | 'administracion' | 'actividad'

const entities = ['branches', 'roles', 'users', 'memberships', 'lockers', 'compartments', 'locks', 'cameras', 'tools', 'placements', 'authorizations', 'loans', 'operations', 'audit']
const labels: Record<string, string> = { branches: 'Sucursales', roles: 'Roles', users: 'Usuarios', memberships: 'Membresías', lockers: 'Lockers', compartments: 'Compartimientos', locks: 'Cerraduras', cameras: 'Cámaras', tools: 'Herramientas', placements: 'Ubicaciones', authorizations: 'Autorizaciones', loans: 'Préstamos', operations: 'Operaciones', audit: 'Auditoría' }
const nav: [View, string, string][] = [['inicio', '⌂', 'Inicio'], ['operacion', '↗', 'Operación'], ['operaciones', '⟳', 'Operaciones'], ['inventario', '▦', 'Inventario'], ['lockers', '▣', 'Lockers'], ['administracion', '⚙', 'Administración'], ['actividad', '◷', 'Actividad']]
// Administrative states the backend accepts per entity (#19); domain-only
// states (tools ON_LOAN) are deliberately absent and never offered.
const editableStatus: Record<string, string[]> = { branches: ['ACTIVE', 'INACTIVE'], users: ['ACTIVE', 'INACTIVE'], memberships: ['ACTIVE', 'INACTIVE'], lockers: ['ONLINE', 'INACTIVE'], compartments: ['ACTIVE', 'INACTIVE'], locks: ['LOCKED', 'INACTIVE'], cameras: ['SIMULATED', 'INACTIVE'], tools: ['AVAILABLE', 'INACTIVE'] }

async function request(path: string, options?: RequestInit, signal?: AbortSignal) {
  const response = await fetch(`/api/v1${path}`, {
    ...options,
    headers: { 'Content-Type': 'application/json', ...(options?.headers as Record<string, string> | undefined) },
    signal,
  })
  if (!response.ok) throw new Error((await response.json().catch(() => ({ detail: 'Error de conexión' }))).detail)
  return response.json() as Promise<Item | Item[]>
}

const text = (value: unknown) => value === null || value === undefined ? '—' : String(value)
const date = (value: unknown) => value ? new Intl.DateTimeFormat('es-PY', { dateStyle: 'short', timeStyle: 'short' }).format(new Date(String(value))) : '—'

export function App() {
  const [loggedIn, setLoggedIn] = useState(false)
  const [view, setView] = useState<View>('inicio')
  const [data, setData] = useState<Data>({})
  const [dashboard, setDashboard] = useState<Item>({})
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<Item | null>(null)
  const [adminEntity, setAdminEntity] = useState('branches')
  // Backend feature detection (F4.1): Python answers 404 for /capabilities,
  // so presence and reconciliation stay off against the Python backend.
  const [caps, setCaps] = useState<Record<string, boolean>>({})
  const [presence, setPresence] = useState<Item[]>([])
  // Lab session token from the login response (never a hardcoded constant);
  // only the reconciliation request carries it.
  const [token, setToken] = useState('')

  const load = async () => {
    try {
      const values = await Promise.all(entities.map((entity) => request(`/${entity}`)))
      setData(Object.fromEntries(entities.map((entity, index) => [entity, values[index] as Item[]])))
      setDashboard(await request('/dashboard') as Item)
      setError('')
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'No se pudo cargar el laboratorio') }
  }
  useEffect(() => {
    if (!loggedIn) return
    const initial = window.setTimeout(() => void load(), 0)
    const timer = window.setInterval(() => void load(), 4000)
    return () => {
      window.clearTimeout(initial)
      window.clearInterval(timer)
    }
  }, [loggedIn])

  useEffect(() => {
    if (!loggedIn) return
    void request('/capabilities').then((found) => setCaps(found as unknown as Record<string, boolean>)).catch(() => setCaps({}))
  }, [loggedIn])

  // Presence rides its own light poll (F4.1-A): only when the backend
  // advertises it, so the Python panel never issues this request.
  useEffect(() => {
    if (!loggedIn || !caps.presence) return
    const loadPresence = async () => {
      try { setPresence(await request('/lockers/presence') as Item[]) } catch (reason) { setError(reason instanceof Error ? reason.message : 'No se pudo cargar la presencia') }
    }
    const initial = window.setTimeout(() => void loadPresence(), 0)
    const timer = window.setInterval(() => void loadPresence(), 4000)
    return () => {
      window.clearTimeout(initial)
      window.clearInterval(timer)
    }
  }, [loggedIn, caps.presence])

  const login = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); const form = new FormData(event.currentTarget)
    try {
      const session = await request('/auth/login', { method: 'POST', body: JSON.stringify({ username: form.get('username'), password: form.get('password') }) }) as Item
      setToken(String(session.access_token ?? ''))
      setLoggedIn(true)
    } catch { setError('Usuario o contraseña incorrectos') }
  }
  const post = async (path: string, body?: Item, method = 'POST') => {
    try { await request(path, { method, body: body ? JSON.stringify(body) : undefined }); setNotice('Operación registrada correctamente.'); await load() }
    catch (reason) { setError(reason instanceof Error ? reason.message : 'No se pudo completar la acción') }
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
      await load()
    } catch (reason_) { setError(reason_ instanceof Error ? reason_.message : 'No se pudo completar la conciliación') }
  }
  const activeTools = data.tools?.filter((tool) => tool.status === 'AVAILABLE') ?? []
  const filteredTools = activeTools.filter((tool) => `${tool.asset_code} ${tool.name}`.toLowerCase().includes(query.toLowerCase()))
  const latest = (resource: string) => data[resource]?.slice(0, 8) ?? []

  if (!loggedIn) return <main className="login-page"><section className="login-card"><p className="brand">LOCKER <span>LAB</span></p><h1>Control de herramientas</h1><p>Acceso administrativo del laboratorio.</p><form onSubmit={login}><label>Usuario<input name="username" defaultValue="admin" required /></label><label>Contraseña<input name="password" type="password" defaultValue="admin" required /></label><button>Ingresar al panel</button></form><small>Laboratorio: admin / admin</small>{error && <p className="message error">{error}</p>}</section></main>

  return <div className="app-shell"><aside><div className="logo">LOCKER <b>LAB</b></div><nav>{nav.map(([key, icon, name]) => <button key={key} className={view === key ? 'selected' : ''} onClick={() => { setView(key); setSelected(null) }}><span>{icon}</span>{name}</button>)}</nav><div className="sidebar-footer"><span className="online-dot" />Laboratorio conectado</div></aside><main className="workspace"><header><div><p className="breadcrumb">Laboratorio / {nav.find((item) => item[0] === view)?.[2]}</p><h1>{nav.find((item) => item[0] === view)?.[2]}</h1></div><div className="user">admin <button onClick={() => setLoggedIn(false)}>Salir</button></div></header>{error && <p className="message error">{error}<button onClick={() => setError('')}>×</button></p>}{notice && <p className="message notice">{notice}<button onClick={() => setNotice('')}>×</button></p>}
    {view === 'inicio' && <Dashboard dashboard={dashboard} latest={latest('audit')} presence={presence} operations={data.operations ?? []} loans={data.loans ?? []} />}
    {view === 'inventario' && <Inventory tools={filteredTools} query={query} setQuery={setQuery} selected={selected} setSelected={setSelected} />}
    {view === 'lockers' && <Lockers lockers={data.lockers ?? []} presence={presence} onSelect={setSelected} selected={selected} />}
    {view === 'operacion' && <Operations data={data} post={post} />}
    {view === 'operaciones' && <OperationsPanel data={data} caps={caps} resolve={resolve} />}
    {view === 'administracion' && <Administration entity={adminEntity} setEntity={setAdminEntity} data={data} post={post} />}
    {view === 'actividad' && <Activity audit={data.audit ?? []} authorizations={data.authorizations ?? []} loans={data.loans ?? []} />}
  </main></div>
}

function Dashboard({ dashboard, latest, presence, operations, loans }: { dashboard: Item; latest: Item[]; presence: Item[]; operations: Item[]; loans: Item[] }) {
  const cards = [['Lockers', dashboard.lockers], ['Herramientas disponibles', dashboard.tools_available], ['En préstamo', dashboard.tools_on_loan], ['Operaciones pendientes', dashboard.pending_loans]]
  // Never claim "all set" while something needs an operator (F4.1-A): an
  // uncertain/failed operation holding a loan, or an administratively-online
  // locker without observed presence.
  const loanStatus = (id: unknown) => text(loans.find((loan) => loan.id === id)?.status)
  const interventions = operations.filter((op) => op.status === 'UNKNOWN' || (op.status === 'FAILED' && ['CHECKOUT_PENDING', 'RETURN_PENDING'].includes(loanStatus(op.loan_id))))
  const offline = presence.filter((row) => row.administrative_status === 'ONLINE' && row.presence !== 'ONLINE')
  const attention = interventions.length + offline.length
  return <><section className={attention ? 'hero warn' : 'hero'}><div><p>ESTADO DEL LABORATORIO</p><h2>{attention ? 'Atención requerida' : 'Todo listo para operar'}</h2><span>{attention ? `${interventions.length} operación(es) por conciliar, ${offline.length} locker(s) sin presencia confirmada.` : 'Los datos se actualizan automáticamente.'}</span></div><div className="hero-mark">{attention ? '!' : '✓'}</div></section><section className="cards">{cards.map(([label, value]) => <article key={String(label)}><small>{label}</small><strong>{text(value)}</strong></article>)}</section><section className="panel"><h2>Actividad reciente</h2><Table items={latest} columns={['action', 'entity_type', 'created_at']} /></section></>
}
function Inventory({ tools, query, setQuery, selected, setSelected }: { tools: Item[]; query: string; setQuery: (v: string) => void; selected: Item | null; setSelected: (v: Item) => void }) {
  // History of the tool on screen only; state lands in async callbacks so a
  // late answer from a previous selection can never overwrite it (#14).
  const [historyState, setHistoryState] = useState<{ id: string; items: Item[]; error: string }>({ id: '', items: [], error: '' })
  useEffect(() => {
    const id = selected?.id ? String(selected.id) : ''
    if (!id) return
    const controller = new AbortController()
    void request(`/tools/${id}/history`, undefined, controller.signal)
      .then((items) => setHistoryState({ id, items: items as Item[], error: '' }))
      .catch((reason: unknown) => {
        if (reason instanceof DOMException && reason.name === 'AbortError') return
        setHistoryState({ id, items: [], error: reason instanceof Error ? reason.message : 'No se pudo cargar el historial' })
      })
    return () => controller.abort()
  }, [selected?.id])
  const history = selected?.id && historyState.id === String(selected.id) ? historyState.items : []
  const historyError = selected?.id && historyState.id === String(selected.id) ? historyState.error : ''
  const active = history.find((placement) => !placement.removed_at)
  return <div className="split"><section className="panel"><div className="panel-title"><h2>Herramientas</h2><input placeholder="Buscar herramienta…" value={query} onChange={(event) => setQuery(event.target.value)} /></div><Table items={tools} columns={['asset_code', 'name', 'status']} onSelect={setSelected} /></section><section className="panel detail"><h2>Detalle de herramienta</h2>{historyError && <p className="message error">{historyError}<button onClick={() => setHistoryState({ id: '', items: [], error: '' })}>×</button></p>}{selected ? <><b>{text(selected.name)}</b><p>Código: {text(selected.asset_code)}</p><p>Estado: <Badge value={text(selected.status)} /></p><p>RFID: {text(selected.rfid_tag)}</p><p>Ubicación actual: {active ? text(active.compartment_id) : '—'}</p><h3>Historial de ubicaciones</h3>{history.length ? <Table items={history} columns={['locker_id', 'compartment_id', 'placed_at', 'removed_at']} /> : <p>Sin movimientos registrados.</p>}</> : <p>Seleccione una herramienta para consultar su historial.</p>}</section></div>
}
function Lockers({ lockers, presence, selected, onSelect }: { lockers: Item[]; presence: Item[]; selected: Item | null; onSelect: (item: Item) => void }) {
  const [detail, setDetail] = useState<Item | null>(null)
  useEffect(() => { if (selected?.id) void request(`/lockers/${selected.id}/detail`).then((item) => setDetail(item as Item)) }, [selected])
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
  return <div className="split"><section className="panel"><h2>Lockers</h2><Table items={rows} columns={['code', 'name', 'status', ...presenceColumns]} onSelect={onSelect} /></section><section className="panel"><h2>{selected ? `Locker ${text(selected.code)}` : 'Detalle del locker'}</h2>{selected ? <>{selectedPresence && <p className="presence-line">Presencia observada: <Badge value={text(selectedPresence.presence)} />{text(selectedPresence.ambiguous_code) === 'true' ? ' (código duplicado: no atribuible)' : ` — última comunicación ${date(selectedPresence.last_communication)}`}</p>}<div className="compartments">{compartments.map((item) => <article key={text(item.id)} className={item.tool ? 'occupied' : ''}><b>{text(item.code)}</b><span>{item.tool ? text((item.tool as unknown as Item).asset_code) : 'Libre'}</span><small>{text((item.lock as unknown as Item | undefined)?.status ?? 'Sin cerradura')}</small></article>)}</div></> : <p>Seleccione un locker para ver sus compartimientos.</p>}</section></div>
}
function Operations({ data, post }: { data: Data; post: (p: string, b?: Item) => Promise<void> }) { const [tool, setTool] = useState(''); const [user, setUser] = useState(''); const branch = data.branches?.[0]?.id ?? ''; return <><div className="split"><section className="panel"><h2>Nueva autorización</h2><form onSubmit={(e) => { e.preventDefault(); void post('/authorizations', { tool_id: tool, user_id: user, branch_id: String(branch) }) }}><select value={tool} onChange={(e) => setTool(e.target.value)} required><option value="">Herramienta</option>{data.tools?.filter((i) => i.status === 'AVAILABLE').map((i) => <option value={text(i.id)} key={text(i.id)}>{text(i.asset_code)} — {text(i.name)}</option>)}</select><select value={user} onChange={(e) => setUser(e.target.value)} required><option value="">Usuario</option>{data.users?.filter((i) => i.status === 'ACTIVE').map((i) => <option value={text(i.id)} key={text(i.id)}>{text(i.display_name)}</option>)}</select><button>Crear autorización</button></form></section><section className="panel"><h2>Flujo simulado</h2><p>El panel solicita abrir el compartimiento. El simulador confirma la acción por MQTT y actualiza el préstamo.</p><span className="legend">Actualización automática cada 4 segundos</span></section></div><section className="panel"><h2>Autorizaciones</h2><Table items={data.authorizations ?? []} columns={['status', 'tool_id', 'user_id', 'created_at']} actions={(item) => <>{item.status === 'PENDING' && <button onClick={() => void post(`/authorizations/${item.id}/approve`)}>Aprobar</button>}{item.status === 'APPROVED' && <button onClick={() => void post(`/authorizations/${item.id}/checkout`)}>Retirar</button>}{['PENDING', 'APPROVED'].includes(text(item.status)) && <button className="quiet" onClick={() => void post(`/authorizations/${item.id}/cancel`)}>Cancelar</button>}</>} /></section><section className="panel"><h2>Préstamos</h2><Table items={data.loans ?? []} columns={['status', 'tool_id', 'user_id', 'checked_out_at', 'returned_at']} actions={(item) => item.status === 'ACTIVE' ? <button onClick={() => void post(`/loans/${item.id}/return`)}>Registrar devolución</button> : undefined} /></section></> }
// ---------------------------------------------------------------------------
// F4.1-B: operations supervision and administrative reconciliation.
// ---------------------------------------------------------------------------

/** Client-side eligibility mirrors the backend rule (F4.0): an UNKNOWN, or a
 * FAILED that still holds a pending loan. The backend stays the authority:
 * a 409 from `resolve` is shown as-is. */
const needsIntervention = (operation: Item, loans: Map<string, Item>) =>
  operation.status === 'UNKNOWN'
  || (operation.status === 'FAILED'
    && ['CHECKOUT_PENDING', 'RETURN_PENDING'].includes(text(loans.get(String(operation.loan_id))?.status)))

function OperationsPanel({ data, caps, resolve }: { data: Data; caps: Record<string, boolean>; resolve: (operationId: string, outcome: string, reason: string, evidence: string) => Promise<void> }) {
  const [statusFilter, setStatusFilter] = useState('ALL')
  const [selectedId, setSelectedId] = useState('')
  const byId = (resource: string) => new Map((data[resource] ?? []).map((item) => [String(item.id), item]))
  const tools = byId('tools'), lockers = byId('lockers'), compartments = byId('compartments'), users = byId('users'), loans = byId('loans')
  const rows = (data.operations ?? [])
    .filter((operation) => statusFilter === 'ALL' || operation.status === statusFilter)
    .map((operation): Item => ({
      ...operation,
      tool_name: text(tools.get(String(operation.tool_id))?.name),
      locker_code: text(lockers.get(String(operation.locker_id))?.code),
      compartment_code: text(compartments.get(String(operation.compartment_id))?.code),
      user_name: text(users.get(String(loans.get(String(operation.loan_id))?.user_id))?.display_name),
      attention: needsIntervention(operation, loans) ? '⚠' : '',
    }))
    .sort((a, b) => Number(Boolean(b.attention)) - Number(Boolean(a.attention)))
  const selected = rows.find((row) => String(row.id) === selectedId) ?? null
  const statuses = ['ALL', 'PENDING', 'PUBLISHING', 'PUBLISHED', 'CONFIRMED', 'FAILED', 'UNKNOWN']
  return <div className="split">
    <section className="panel">
      <div className="panel-title"><h2>Operaciones</h2>
        <label className="status-select">Filtro<select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}>{statuses.map((status) => <option key={status} value={status}>{status === 'ALL' ? 'Todos' : status}</option>)}</select></label>
      </div>
      <Table items={rows} columns={['status', 'attention', 'command_type', 'tool_name', 'locker_code', 'compartment_code', 'user_name', 'created_at']} onSelect={(item) => setSelectedId(String(item.id))} />
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
        {caps.resolve && needsIntervention(selected, loans)
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

function Administration({ entity, setEntity, data, post }: { entity: string; setEntity: (v: string) => void; data: Data; post: (p: string, b?: Item, method?: string) => Promise<void> }) { const [payload, setPayload] = useState('{\n  "code": "",\n  "name": ""\n}'); const editable = editableStatus[entity]; return <><section className="panel admin-toolbar"><label>Entidad<select value={entity} onChange={(e) => setEntity(e.target.value)}>{entities.filter((e) => !['authorizations', 'loans', 'operations', 'audit'].includes(e)).map((e) => <option key={e}>{e}</option>)}</select></label><p>Alta, edición y desactivación. Las referencias se introducen como UUID desde los listados relacionados.</p></section><div className="split"><section className="panel"><h2>{labels[entity]}</h2><Table items={data[entity] ?? []} columns={columnsFor(entity)} actions={(item) => <>{editable?.includes(text(item.status)) && <label className="status-select">Estado<select value={text(item.status)} onChange={(e) => void post(`/${entity}/${item.id}`, { status: e.target.value }, 'PATCH')}>{editable.map((s) => <option key={s} value={s}>{s}</option>)}</select></label>}{editable && item.status !== 'INACTIVE' && editable.includes(text(item.status)) && <button className="quiet" onClick={() => void post(`/${entity}/${item.id}/deactivate`)}>Desactivar</button>}</>} /></section><section className="panel"><h2>Crear registro</h2><p>Complete el JSON según la entidad. Las relaciones usan el UUID mostrado en los listados.</p><textarea value={payload} onChange={(e) => setPayload(e.target.value)} rows={10} /><button onClick={() => { try { void post(`/${entity}`, JSON.parse(payload) as Item) } catch { alert('JSON inválido') } }}>Crear</button></section></div></> }
function Activity({ audit, authorizations, loans }: { audit: Item[]; authorizations: Item[]; loans: Item[] }) { return <div className="activity-grid"><section className="panel"><h2>Auditoría</h2><Table items={audit} columns={['action', 'entity_type', 'created_at']} /></section><section className="panel"><h2>Autorizaciones</h2><Table items={authorizations} columns={['status', 'created_at']} /></section><section className="panel"><h2>Préstamos</h2><Table items={loans} columns={['status', 'checked_out_at', 'returned_at']} /></section></div> }
function columnsFor(entity: string) { return ({ branches: ['code', 'name', 'status'], roles: ['code', 'name'], users: ['username', 'display_name', 'status'], memberships: ['user_id', 'branch_id', 'role_id', 'status'], lockers: ['code', 'name', 'branch_id', 'status'], compartments: ['code', 'name', 'position', 'locker_id', 'status'], locks: ['compartment_id', 'hardware_address', 'status'], cameras: ['name', 'locker_id', 'status'], tools: ['asset_code', 'name', 'rfid_tag', 'status'] } as Record<string, string[]>)[entity] ?? ['id', 'status'] }
function Table({ items, columns, actions, onSelect }: { items: Item[]; columns: string[]; actions?: (item: Item) => ReactNode; onSelect?: (item: Item) => void }) { return <div className="table-wrap"><table><thead><tr>{columns.map((c) => <th key={c}>{c.replaceAll('_', ' ')}</th>)}{actions && <th>Acciones</th>}</tr></thead><tbody>{items.length ? items.map((item) => <tr key={text(item.id)} onClick={() => onSelect?.(item)}>{columns.map((column) => <td key={column}>{column.includes('_at') || column === 'last_communication' ? date(item[column]) : column === 'status' || column === 'presence' ? <Badge value={text(item[column])} /> : text(item[column])}</td>)}{actions && <td className="actions" onClick={(e) => e.stopPropagation()}>{actions(item)}</td>}</tr>) : <tr><td colSpan={columns.length + 1}>Sin registros.</td></tr>}</tbody></table></div> }
function Badge({ value }: { value: string }) { return <span className={`badge ${value.toLowerCase()}`}>{value.replaceAll('_', ' ')}</span> }
