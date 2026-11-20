# Línea base de rendimiento — panel LOCKER (F4.0)

Medición previa a las optimizaciones de F4.2 (paginación, carga por vista).
Dataset y condiciones para repetir: laboratorio del seed `LAB` (1 sucursal,
1 locker, 24 compartimientos, 24 herramientas, 3 usuarios), panel React
abierto en un único navegador. Medido el 20-11-2026 en el laboratorio local
(Windows + Docker Desktop/WSL2) con carga sintética equivalente al ciclo del
panel: 14 listados + 1 dashboard cada 4 s contra `http://127.0.0.1:8083`
(a través de nginx, como el navegador).

## Valores calculados (del código, `apps/web/src/App.tsx`)

- `load()` pide **15 solicitudes por ciclo** (14 entidades + dashboard).
- Ciclo cada 4 s → 3,75 req/s → **225 solicitudes/min teóricas**.
- Sin paginación: cada listado transfiere la tabla completa.

## Valores observados

- **225 teóricas vs ~202/min reales** (165 OK, 0 fallos, 49 s): el ciclo
  efectivo dura ~4 s de espera + latencia de las 15 solicitudes.
- **Latencia y tamaño por endpoint** (20 muestras, a través de nginx → API
  Python; promedio / p95 / cuerpo sin comprimir):

  | Endpoint | Promedio | p95 | Tamaño |
  |---|---:|---:|---:|
  | `/api/v1/tools` | 7,4 ms | 8,8 ms | 6,2 KB |
  | `/api/v1/placements` | 8,4 ms | 9,5 ms | 11,1 KB |
  | `/api/v1/compartments` | 6,9 ms | 8,2 ms | 6,0 KB |
  | `/api/v1/locks` | 9,1 ms | 25 ms | 5,9 KB |
  | `/api/v1/audit` | 7,8 ms | 9,2 ms | 5,9 KB |
  | `/api/v1/operations` | 8,8 ms | 9,9 ms | 1,2 KB |
  | `/api/v1/dashboard` | 13,7 ms | 17 ms | 2,8 KB |
  | resto de listados | 6,6–8,5 ms | ≤10,3 ms | 0,2–0,9 KB |

  **~52 KB por ciclo sin comprimir** (suma de los 15 cuerpos), ~780 KB/min.
- **CPU y RAM con ciclo sostenido 3 min** (`docker stats`, 8 muestras cada
  20 s): API Python 0,2–18,6 % (picos por ciclo) y 88–100 MiB; PostgreSQL
  0–3,2 % y ~38 MiB; nginx 0–2,4 % y ~15 MiB; Mosquitto ~0,04 % y ~8 MiB;
  simulador ~0,01 % y ~16 MiB. En reposo (sin ciclo): API 0,33 % / 88 MiB.
  Ningún contenedor mostró crecimiento sostenido de memoria en la ventana.
- El access log de nginx no escribe en `docker logs` (configuración
  preexistente del contenedor); el conteo de solicitudes se hizo con el
  generador propio, que registró 0 fallos en toda la ventana.

## Limitaciones

- Un solo cliente y host; el navegador real añade render, pero no cambia el
  patrón de red medido. Ventana de 3 min, no horas.
- El stack del laboratorio corría imágenes pre-F4.0: F4.0 no altera los
  caminos de lectura medidos (solo añade validación en escrituras), y el
  ciclo de sondeo del panel no cambió (la reducción de cargas es F4.2).
- Sin compresión medida por separado: nginx no declara `gzip` para
  `/api/v1`; los tamaños son los cuerpos HTTP tal como se transfieren.

## Qué debe comparar F4.2

Mismo procedimiento, mismo seed: (1) solicitudes/min con carga por vista,
(2) pares latencia/tamaño de los endpoints que sobrevivan a la paginación,
(3) CPU/RAM de API y PostgreSQL en la misma ventana. Un éxito de F4.2 es
 reducir los 15×ciclo a ≤3 por vista activa y el ~52 KB/ciclo proporcionalmente.

## Delta F4.1 (medido sobre el preview aislado Rust)

- El ciclo del panel contra **Python** queda en 15 solicitudes (sin
  presencia ni conciliación: `GET /capabilities` responde 404 y el panel no
  vuelve a pedirlas en el ciclo).
- Contra **Rust** el ciclo pasa a 16: `GET /api/v1/lockers/presence`
  añade **6,4 ms promedio / 23,3 ms p95 / 336 B** (estado calculado en
  memoria, sin consultas por locker). `GET /capabilities` (~3 ms / 32 B) se
  pide una única vez tras el login, no por ciclo.
- La vista de Operaciones no añade solicitudes: enriquece filas con las
  entidades ya sondeadas.
