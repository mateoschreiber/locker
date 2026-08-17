import { useEffect, useState } from 'react'

type SystemInfo = {
  name: string
  version: string
  environment: string
  timezone: string
  status: string
}

export function App() {
  const [info, setInfo] = useState<SystemInfo | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    fetch('/api/v1/system/info')
      .then((response) => {
        if (!response.ok) throw new Error('API unavailable')
        return response.json() as Promise<SystemInfo>
      })
      .then(setInfo)
      .catch(() => setError(true))
  }, [])

  return (
    <main>
      <section aria-labelledby="title">
        <p className="eyebrow">LABORATORIO</p>
        <h1 id="title">Locker Lab</h1>
        <p className="description">Base de simulación para control inteligente de herramientas.</p>
        {info ? (
          <dl>
            <div><dt>Estado</dt><dd>{info.status}</dd></div>
            <div><dt>Entorno</dt><dd>{info.environment}</dd></div>
            <div><dt>Zona horaria</dt><dd>{info.timezone}</dd></div>
            <div><dt>Versión</dt><dd>{info.version}</dd></div>
          </dl>
        ) : (
          <p className={error ? 'error' : 'pending'}>
            {error ? 'No se pudo consultar la API del laboratorio.' : 'Comprobando servicios…'}
          </p>
        )}
      </section>
    </main>
  )
}
