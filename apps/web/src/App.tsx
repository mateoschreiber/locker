import { FormEvent, useEffect, useState } from 'react'

type SystemInfo = {
  name: string
  version: string
  environment: string
  timezone: string
  status: string
}

type RecordItem = Record<string, string | null>

const resources = ['branches', 'users', 'lockers', 'compartments', 'locks', 'cameras', 'tools', 'placements', 'authorizations', 'loans']

export function App() {
  const [info, setInfo] = useState<SystemInfo | null>(null)
  const [error, setError] = useState(false)
  const [loggedIn, setLoggedIn] = useState(false)
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('admin')
  const [data, setData] = useState<Record<string, RecordItem[]>>({})
  const [branchCode, setBranchCode] = useState('')
  const [branchName, setBranchName] = useState('')
  const [toolCode, setToolCode] = useState('')
  const [toolName, setToolName] = useState('')

  const loadData = () => {
    Promise.all(resources.map((resource) => fetch(`/api/v1/${resource}`).then((response) => response.json() as Promise<RecordItem[]>)))
      .then((responses) => setData(Object.fromEntries(resources.map((resource, index) => [resource, responses[index]]))))
      .catch(() => setError(true))
  }

  useEffect(() => {
    fetch('/api/v1/system/info')
      .then((response) => {
        if (!response.ok) throw new Error('API unavailable')
        return response.json() as Promise<SystemInfo>
      })
      .then(setInfo)
      .catch(() => setError(true))
  }, [])

  const login = (event: FormEvent) => {
    event.preventDefault()
    fetch('/api/v1/auth/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username, password }) })
      .then((response) => {
        if (!response.ok) throw new Error('login failed')
        setLoggedIn(true)
        loadData()
      })
      .catch(() => setError(true))
  }

  const create = (path: string, payload: Record<string, string>, reset: () => void) => {
    fetch(`/api/v1/${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) })
      .then((response) => {
        if (!response.ok) throw new Error('create failed')
        reset()
        loadData()
      })
      .catch(() => setError(true))
  }

  return (
    <main>
      <section aria-labelledby="title">
        <p className="eyebrow">LABORATORIO</p>
        <h1 id="title">Locker Lab</h1>
        <p className="description">Base de simulación para control inteligente de herramientas.</p>
        {!loggedIn ? (
          <form className="login" onSubmit={login}>
            <label>Usuario<input value={username} onChange={(event) => setUsername(event.target.value)} /></label>
            <label>Contraseña<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} /></label>
            <button type="submit">Ingresar</button>
            <small>Laboratorio: admin / admin</small>
          </form>
        ) : info ? (
          <>
          <dl>
            <div><dt>Estado</dt><dd>{info.status}</dd></div>
            <div><dt>Entorno</dt><dd>{info.environment}</dd></div>
            <div><dt>Zona horaria</dt><dd>{info.timezone}</dd></div>
            <div><dt>Versión</dt><dd>{info.version}</dd></div>
          </dl>
          <div className="forms">
            <form onSubmit={(event) => { event.preventDefault(); create('branches', { code: branchCode, name: branchName }, () => { setBranchCode(''); setBranchName('') }) }}>
              <h2>Nueva sucursal</h2><input placeholder="Código" value={branchCode} onChange={(event) => setBranchCode(event.target.value)} required /><input placeholder="Nombre" value={branchName} onChange={(event) => setBranchName(event.target.value)} required /><button>Crear</button>
            </form>
            <form onSubmit={(event) => { event.preventDefault(); create('tools', { asset_code: toolCode, name: toolName }, () => { setToolCode(''); setToolName('') }) }}>
              <h2>Nueva herramienta</h2><input placeholder="Código" value={toolCode} onChange={(event) => setToolCode(event.target.value)} required /><input placeholder="Nombre" value={toolName} onChange={(event) => setToolName(event.target.value)} required /><button>Crear</button>
            </form>
          </div>
          <div className="resources">{resources.map((resource) => <section key={resource}><h2>{resource}</h2><strong>{data[resource]?.length ?? 0}</strong><ul>{data[resource]?.slice(0, 4).map((item) => <li key={item.id}>{item.name ?? item.code ?? item.asset_code ?? item.username ?? item.status}</li>)}</ul></section>)}</div>
          </>
        ) : (
          <p className={error ? 'error' : 'pending'}>
            {error ? 'No se pudo consultar la API del laboratorio.' : 'Comprobando servicios…'}
          </p>
        )}
      </section>
    </main>
  )
}
