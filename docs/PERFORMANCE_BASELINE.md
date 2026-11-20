# Línea base de rendimiento �?" panel LOCKER (F4.0)

Medición previa a las optimizaciones de F4.2 (paginación, carga por vista).
Dataset y condiciones para repetir: laboratorio del seed `LAB` (1 sucursal,
1 locker, 24 compartimientos, 24 herramientas, 3 usuarios), panel React
abierto en un único navegador. Medido el 20-11-2026 en el laboratorio local
(Windows + Docker Desktop/WSL2) con carga sintética equivalente al ciclo del
panel: 14 listados + 1 dashboard cada 4 s contra `http://127.0.0.1:8083`
(a través de nginx, como el navegador).

## Valores calculados (del código, `apps/web/src/App.tsx`)

- `load()` pide **15 solicitudes por ciclo** (14 entidades + dashboard).
- Ciclo cada 4 s �?' 3,75 req/s �?' **225 solicitudes/min teóricas**.
- Sin paginación: cada listado transfiere la tabla completa.

## Valores observados

- **225 teóricas vs ~202/min reales** (165 OK, 0 fallos, 49 s): el ciclo
  efectivo dura ~4 s de espera + latencia de las 15 solicitudes.
- **Latencia y tamaño por endpoint** (20 muestras, a través de nginx �?' API
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
  | resto de listados | 6,6�?"8,5 ms | �?�10,3 ms | 0,2�?"0,9 KB |

  **~52 KB por ciclo sin comprimir** (suma de los 15 cuerpos), ~780 KB/min.
- **CPU y RAM con ciclo sostenido 3 min** (`docker stats`, 8 muestras cada
  20 s): API Python 0,2�?"18,6 % (picos por ciclo) y 88�?"100 MiB; PostgreSQL
  0�?"3,2 % y ~38 MiB; nginx 0�?"2,4 % y ~15 MiB; Mosquitto ~0,04 % y ~8 MiB;
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
 reducir los 15�-ciclo a �?�3 por vista activa y el ~52 KB/ciclo proporcionalmente.

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

## Baseline Rust (F4.2-C, medido antes de optimizar; 20-11-2026)

Mismo procedimiento que la línea base Python, sobre el preview aislado
Rust desechable (`scripts/bench_panel.py`, proyecto propio, puerto 8086,
seed LAB fresco, simulador activo): 16 solicitudes en paralelo cada 4 s
durante 3 min a través del nginx del preview, 46 ciclos, **0 fallos**,
245 req/min observadas (240 teóricas).

- **El seed fresco explica los tamaños pequeños**: préstamos,
  autorizaciones, operaciones y auditoría arrancan vacíos (~0,0 KB), a
  diferencia del laboratorio con historia de F4.0.
- **Latencia y tamaño por endpoint** (46 muestras, promedio / p95 / cuerpo):

  | Endpoint | Promedio | p95 | Tamaño |
  |---|---:|---:|---:|
  | `/api/v1/placements` | 11,3 ms | 14,9 ms | 10,4 KB |
  | `/api/v1/tools` | 11,1 ms | 15,2 ms | 6,0 KB |
  | `/api/v1/compartments` | 14,0 ms | 16,0 ms | 5,9 KB |
  | `/api/v1/locks` | 13,6 ms | 14,7 ms | 5,7 KB |
  | `/api/v1/users` | 12,2 ms | 12,9 ms | 0,6 KB |
  | `/api/v1/memberships` | 12,3 ms | 12,3 ms | 0,9 KB |
  | `/api/v1/dashboard` | 11,8 ms | 15,2 ms | 0,1 KB |
  | `/api/v1/lockers` | 11,9 ms | 10,6 ms | 0,3 KB |
  | `/api/v1/branches` | 13,5 ms | 12,3 ms | 0,2 KB |
  | `/api/v1/roles` | 12,6 ms | 12,1 ms | 0,5 KB |
  | `/api/v1/cameras` | 12,4 ms | 14,5 ms | 0,2 KB |
  | `/api/v1/operations` | 9,5 ms | 13,3 ms | 0,0 KB |
  | `/api/v1/loans` | 10,7 ms | 19,7 ms | 0,0 KB |
  | `/api/v1/authorizations` | 10,4 ms | 14,9 ms | 0,0 KB |
  | `/api/v1/audit` | 10,1 ms | 15,0 ms | 0,0 KB |
  | `/api/v1/lockers/presence` | 10,1 ms | 13,5 ms | 0,3 KB |

  **~31 KB por ciclo**, ~0,5 MB/min. La ráfaga paralela de 16 (como el
  navegador) explica los promedios ~10-14 ms, superiores a la medición
  secuencial de F4.0 contra Python.
- **CPU y RAM** (docker stats, reposo + 8 muestras cada 20 s): api-rust
  0,16-2,08 % / 3,4-5,4 MiB; PostgreSQL 0,16-3,8 % / 46-52 MiB; nginx
  0-1,0 % / 11-14 MiB; mosquitto ~0,04 % (picos de 2,5 % de muestreo) /
  ~2 MiB; simulador ~0,01 % / 16 MiB. Sin crecimiento sostenido.
- Punto de comparación de F4.2: carga por vista activa (�?�3 solicitudes
  dinámicas), mismo seed, mismo procedimiento.

### Escenarios de crecimiento (mismo stack, seed sintético desechable)

`scripts/bench_panel.py inflate` inserta filas sintéticas con la misma forma
que las reales (solo en la base desechable del preview del bench). El ciclo
sin paginar escala linealmente en bytes y degrada latencia y RAM de api-rust
(serialización de JSON completo en cada sondeo):

| Dataset | tools / operaciones / auditoría | Ciclo (16 req) | Audit (prom/p95) | Operaciones | Bytes por ciclo |
|---|---|---|---|---|---:|
| 1x (seed LAB) | 24 / 0 / ~30 | ver arriba | 10,1 / 15,0 ms | 9,5 / 13,3 ms | ~31 KB |
| 10x | 264 / 240 / 2 400 | 245 req/min, 0 fallos | 34,0 / 43,4 ms | 15,5 / 19,1 ms | ~806 KB (~11,8 MB/min) |
| 100x | 2 424 / 2 400 / 24 000 | 245 req/min, 0 fallos | **227,5 / 264,6 ms** | 56,0 / 83,1 ms | **~8 006 KB (~117 MB/min)** |

- 100x: `/api/v1/audit` transfiere **6,0 MB por respuesta**; `/api/v1/tools`
  0,63 MB y `/api/v1/operations` 1,3 MB. api-rust pasa de ~5 MiB (1x) a
  80-129 MiB durante el ciclo (picos de CPU de ~22 % por serialización);
  PostgreSQL se mantiene plano (~57 MiB, �?�4 %): el costo es de transferencia
  y serialización, no de consultas (seq scan sobre tablas chicas en memoria).
- Los datos crecientes confirman el diseño de F4.2: sin paginación, cada
  sondeo del panel paga el historial completo; con `limit=100` el costo deja
  de crecer con el dataset. Sin índices nuevos: a esta escala PostgreSQL no
  es el cuello de botella medido.

## Delta F4.2 (post-optimización; 20-11-2026, mismo stack, mismo seed, mismo procedimiento)

Medido con `scripts/bench_panel.py cycle --view` (ciclos de la vista activa,
ráfaga paralela cada 4 s a través del nginx del preview desechable), sobre
la imagen reconstruida con F4.2. **Todas las cifras de esta sección son
mediciones HTTP sintéticas** — miden red y API, no el render del navegador:
la carga inicial y la respuesta al cambio de vista en un navegador real
quedan como observación manual pendiente.

- **Solicitudes por vista (objetivo ≤3):** inicio 3 (`dashboard`,
  `operations?attention=1&limit=100`, `presence`) · operacion 3
  (`authorizations`, `loans`, herramientas disponibles paginadas) ·
  operaciones 1 · inventario 1 · lockers 2 · actividad 3 · administracion 0
  (solo al entrar y tras mutaciones). Las referencias (sucursales, roles,
  usuarios, lockers, compartimientos) se cargan al entrar y tras cada
  mutación, no en el ciclo. **45–48 req/min por vista vs 245/min** del ciclo
  completo pre-F4.2 (calculado y observado).
- **Dataset 1x (seed LAB):** ≤3 solicitudes por ciclo y **≤6,1 KB por
  ciclo** (vs ~31 KB × 16 solicitudes): herramientas disponibles 6,1 KB /
  15,4 ms promedio; presence 0,3 KB / 11,9 ms; dashboard 0,1 KB / 15,8 ms.
- **Dataset 10x (264/240/2 400):** auditoría paginada **25,2 KB / 11,7 ms
  promedio / 13,0 ms p95** (sin paginar: 582,5 KB / 34,0 / 43,4 ms);
  operaciones enriquecidas 72,1 KB / 23,3 / 31,9 ms (una sola consulta;
  antes: 129,5 KB + 5 tablas de referencia sondeadas); herramientas 25,8 KB
  / 15,3 / 27,5 ms (antes 65,8 KB).
- **Dataset 100x (2 424/2 400/24 000):** auditoría paginada (ventana 3 min,
  46 ciclos) **25,3 KB / 14,6 ms promedio / 28,7 ms p95** — sin paginar en
  el mismo dataset: 6 034,9 KB / 227,5 / 264,6 ms (**p95 −89 %, bytes
  −99,6 %**). Operaciones 72,1 KB / 17,2 / 34,7 ms (antes 1 316 KB / 56,0 /
  83,1); herramientas 26,0 KB / 12,6 / 26,6 ms (antes 626,9 KB / 36,6 /
  49,2). La página paginada no crece con el dataset; su latencia sube de
  11,7 → 14,6 ms (orden de 24 000 filas sin índice, aún barato).
- **CPU y RAM (100x, `docker stats`):** api-rust 0,15–0,74 % / **5,4–6,2 MiB
  planos** (pre-F4.2 en el mismo dataset: picos de ~22 % y 80–129 MiB
  serializando el historial completo); PostgreSQL ≤4,4 % / ~60 MiB. El costo
  pre-F4.2 era de serialización/transferencia, no de consultas — la
  paginación lo elimina.
- Diferencias de método, declaradas: las ventanas post son de 1 min (16
  ciclos) salvo `actividad` a 100x (3 min, 46 ciclos, igual que el
  baseline); el baseline de 16 solicitudes y el ciclo por vista son dos
  comportamientos distintos del panel y así se comparan (la comparación
  por-endpoint usa el mismo dataset y procedimiento).
