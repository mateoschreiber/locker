# Resumen de auditoría técnica — LOCKER

Fecha: 19 de noviembre de 2026. Revisión del repositorio en el commit
`656d31a` (cierre de la Fase 3 de la migración a Rust). Este resumen actualiza
la auditoría original (commit `cb558b3`) conservando la numeración de
hallazgos, que el Plan Maestro y la documentación referencian.

## Diagnóstico

LOCKER es un laboratorio para gestionar y simular la custodia y el préstamo de
herramientas mediante lockers electrónicos. Dispone de un panel web, dos
backends (Python activo y Rust en paralelo, probado de forma aislada),
PostgreSQL, MQTT y un simulador; no controla hardware físico. La carga
esperada es un panel de un administrador con refresco cada 4 segundos
(~15 solicitudes por ciclo) y decenas de operaciones por día.

**Madurez:** prototipo de laboratorio con el camino Rust endurecido
(transacciones, idempotencia, entrega MQTT durable con recuperación y
conciliación, pruebas aisladas). Python sigue siendo el backend activo y
conserva varios de los defectos originales hasta su retirada (Fase 4). No está
preparado para custodiar activos reales: los riesgos P0 de seguridad siguen
abiertos por decisión documentada (Fase 5).

## Arquitectura y funcionamiento actual

```text
Navegador → Nginx y React → FastAPI (activo) → PostgreSQL (lab)
                               ↕ MQTT (Mosquitto) ↔ Simulador
                Rust (perfil opcional) → PostgreSQL aislada (misma API /api/v1)
```

- La Fase 3 migró el puente MQTT a Rust: reclamo atómico de `PENDING`
  (`origin='rust'`), `PENDING → PUBLISHING → PUBLISHED → CONFIRMED/FAILED` con
  `UNKNOWN` para lo indeterminado, conciliación administrativa auditable y
  propiedad exclusiva por operación entre backends (partición por `origin` y
  por estados). PUBACK nunca significa apertura física.
- Las pruebas ahora cubren flujos completos, concurrencia, duplicados,
  caídas de broker, reinicios reales de proceso, migraciones incrementales y
  coexistencia Python/Rust sobre una base compartida; CI ejecuta 8 jobs.
- PostgreSQL mantiene índices únicos parciales de ubicación activa; el resto
  de invariantes las garantizan las transacciones del backend.

## Matriz de hallazgos — estado actual

| Nº | Prioridad | Hallazgo | Estado (19-11-2026) | Evidencia actual |
|---:|:---:|---|---|---|
| 1 | P0 | Rutas sin sesión ni permisos; token fijo. | **Pendiente (F5).** Rust exige `Bearer lab-admin-token` solo en `/operations/{id}/resolve`; todo lo demás sigue abierto en ambos backends. | [login](apps/api/app/modules/routes.py#L77), [resolve](apps/api-rust/src/operations.rs#L378) |
| 2 | P0 | Contraseñas en texto claro y expuestas en respuestas. | **Parcial.** Rust nunca serializa `password`; **Python todavía la devuelve en cada respuesta de usuario, incluido el login** (2 líneas lo corrigen: excluir la columna en `dump`). El almacenamiento en claro y la ausencia de HTTPS siguen hasta F5. | [dump](apps/api/app/modules/routes.py#L36), [fix Rust](apps/api-rust/src/dump.rs#L13) |
| 3 | P0 | MQTT anónimo, sin identidad de dispositivo ni TLS. | **Pendiente (F5).** `allow_anonymous true`; el protocolo exige al hardware futuro deduplicación persistente y rechazo de correlaciones desconocidas. | [mosquitto.conf](infra/docker/mosquitto.conf#L3), [protocolo](docs/hardware/HARDWARE_PROTOCOL.md) |
| 4 | P0 | Seed automático en cada arranque. | **Resuelto (F0).** El seed es explícito, rechaza bases no vacías y el arranque no lo ejecuta. | [seed](apps/api/app/modules/seed.py#L21), [Dockerfile CMD](apps/api/Dockerfile) |
| 5 | P0 | Una apertura simulada no demuestra entrega. | **Mantenido por diseño, endurecido (F3).** PUBACK≠física documentado; `UNKNOWN` y conciliación separan lo incierto; la evidencia física es F5. | [protocolo](docs/hardware/HARDWARE_PROTOCOL.md#ciclo-de-vida-de-una-operación-fase-3) |
| 6 | P1 | Envío no durable: operación guardada antes de publicar sin recuperación. | **Resuelto en Rust (F3).** Outbox + reclamo atómico + `published_at` + recuperación y TTL. **Python sigue fire-and-forget**: un reinicio entre commit y publish deja `PENDING` colgada (se resuelve al retirar Python, F4). | [puente Rust](apps/api-rust/src/mqtt.rs#L133), [Python](apps/api/app/modules/mqtt.py#L59) |
| 7 | P1 | Retiros concurrentes duplicados. | **Resuelto (F2).** `FOR UPDATE` sobre autorización y herramienta, un préstamo activo por herramienta, pruebas concurrentes. | [checkout](apps/api-rust/src/operations.rs#L123) |
| 8 | P1 | Jerarquía sin validar; devolución sin reserva de compartimento. | **Resuelto en Rust (F2).** Jerarquía validada, compartimento reservado u ocupado rechazado, bloqueo de fila del compartimento. Python conserva el comportamiento original hasta F4. | [placements](apps/api-rust/src/inventory.rs#L45), [return](apps/api-rust/src/operations.rs#L243) |
| 9 | P1 | Código de locker único solo por sucursal; el tópico lo usa. | **Resuelto en Rust (F3).** El publicador rechaza códigos ambiguos (`FAILED` sin publicar). La unicidad global de esquema se decidirá con la retirada de Python. | [publicador](apps/api-rust/src/mqtt.rs#L180) |
| 10 | P1 | Auditoría sin actor. | **Parcial.** Rust registra actor (sesión admin del laboratorio) en todo; Python sigue sin actor. Atribución real de sesiones: F5. | [audit Rust](apps/api-rust/src/audit.rs#L7), [audit Python](apps/api/app/modules/routes.py#L49) |
| 11 | P1 | CRUD genérico sin validación de contratos. | **Resuelto en Rust (F2).** Campos, tipos, longitudes, UUID, FK→422, duplicado→409. Python conserva el 500 en payloads inválidos hasta F4. **Véase #19: el valor de `status` sigue siendo texto libre.** | [entities](apps/api-rust/src/entities.rs#L1) |
| 12 | P1 | Identidad MQTT compartida y consumidor duplicado. | **Resuelto en Rust (F3).** `client_id` propio + partición por `origin` y estados, probado en coexistencia sobre una base compartida. Python mantiene el `client_id` fijo (réplicas Python seguirían expulsándose). | [coexistencia](scripts/test_rust_mqtt.py), [puente](apps/api-rust/src/mqtt.rs#L40) |
| 13 | P2 | 14 colecciones por ciclo, dashboard cada 4 s, sin paginación. | **Pendiente (F4).** | [panel](apps/web/src/App.tsx#L43) |
| 14 | P2 | Historial puede incluir ubicaciones de otros activos. | **Parcial.** El filtro por `tool_id` existe, pero conserva una condición muerta (`active.get(selected.id)?.tool_id === selected.id` es siempre cierta si el registro existe); falta la prueba con dos activos. | [filtro](apps/web/src/App.tsx#L76) |
| 15 | P2 | Presencia mostrada sin consumir latidos. | **Pendiente (F4).** El simulador publica `LOCKER_HEARTBEAT`; nadie lo consume. | [simulador](apps/locker-simulator/locker_simulator/__main__.py#L24) |
| 16 | P2 | Falta de pruebas de flujos críticos. | **Muy avanzado.** Suites aisladas PostgreSQL/Mosquitto (Rust 19/19, Python 9/9, simulador 7/7), migraciones, paridad de contratos, escenarios Docker de reinicio y caída, CI con 8 jobs. Faltan pruebas de permisos (F5) y de interfaz profunda. | [ci.yml](.github/workflows/ci.yml) |
| 17 | P2 | Fallos sin vía de recuperación del préstamo. | **Resuelto en Rust (F3/3.6).** `FAILED`/`UNKNOWN` con conciliación auditable que revierte el préstamo; eventos tardíos con reglas definidas. Python sin conciliación hasta F4. | [resolve](apps/api-rust/src/operations.rs#L378) |
| 18 | P3 | Esquemas sin uso (12 clases). | **Pendiente (F4).** Se retiran junto con Python. | [schemas](apps/api/app/modules/schemas.py) |

## Hallazgos nuevos (revisión 19-11-2026)

| Nº | Prioridad | Hallazgo | Acción recomendada | Evidencia |
|---:|:---:|---|---|---|
| 19 | P1 | **El estado de cualquier entidad administrada es texto libre**: `PATCH /{entity}/{id}` acepta cualquier cadena ≤32 caracteres en ambos backends, y el panel lo invita con un `prompt()` de "Estado nuevo". Un estado como `BANANA` saca la herramienta de todos los filtros y del dashboard sin ninguna transición de dominio. | Validar contra la lista de estados permitidos por entidad (422 en caso inválido) y una prueba de contrato; el panel debería ofrecer la lista, no texto libre. | [spec de estado](apps/api-rust/src/entities.rs#L65), [panel](apps/web/src/App.tsx#L79) |
| 20 | P2 | El puente Rust, contra una base sin esquema (perfil de laboratorio), emite un error de reclamo cada 250 ms de forma indefinida. Inofensivo pero ruidoso. | Retroceso exponencial o silencio para el error `42P01` del reclamo; el laboratorio del perfil `rust` no crea esquema automáticamente. | [reclamo](apps/api-rust/src/mqtt.rs#L159) |
| 21 | P3 | Tres jobs de CI (`compose`, `parity`, `mqtt`) reconstruyen las mismas imágenes Docker en cada ejecución. | Compartir artefactos o consolidar los escenarios; hoy es solo tiempo de CI, sin efecto funcional. | [ci.yml](.github/workflows/ci.yml#L84) |

## Hardware y operación

MQTT sigue siendo la frontera correcta. Para cerraduras reales faltan:
identidad autenticada por dispositivo, TLS, correspondencia
compartimento–dirección física (`hardware_address` existe y no se usa),
evidencia del movimiento y deduplicación persistente (la del simulador es una
ventana acotada de 10 000 correlaciones, perdida al reiniciar, documentada
como caché y no como garantía). El ciclo Rust distingue orden solicitada,
publicación aceptada por el broker, apertura confirmada e incierto conciliable;
el panel debe reflejar esa distinción (F4).

## Rendimiento y frontend

Sin cambios medidos: el panel sigue pidiendo ~15 colecciones por ciclo de 4 s
y los listados siguen sin paginar (calculado del código, no medido). La Fase 4
debe medir CPU, memoria y latencia con la misma carga antes de decidir
optimizaciones. El barrido TTL de Rust ahora es cada 5 s (antes cada 250 ms).

## Migración a Rust — estado

Fases 0–3 completadas localmente y **validadas en CI** el 19-11-2026 en
`37026b0` (run #18, 8/8 jobs aprobados:
https://github.com/mateoschreiber/locker/actions/runs/44599990488). Pendiente:
el cutover de la Fase 4, que retirará Python solo tras verificar paridad y
resolver #13–#15. La regla de convivencia vigente: un único dueño por
operación; nunca apuntar ambos backends a la misma base mutable fuera de las
pruebas de coexistencia.

## Orden recomendado (actualizado)

1. Cerrar #19 (estados permitidos) y el volcado de contraseñas en Python
   (2 líneas, parte de #2) — pequeños y anteriores a cualquier Fase 4.
2. ~~Validar CI remoto de la Fase 3~~ — hecho: run #18 sobre `37026b0`,
   8/8 jobs aprobados.
3. Fase 4: latidos y presencia (#15), panel de conciliación, paginación y
   carga por vista (#13, #14), medición de recursos, cutover y retirada de
   Python (#6, #17, #18 se cierran solos al retirarlo).
4. Fase 5: autenticación y permisos (#1), hashes y HTTPS (#2), MQTT con TLS e
   identidad por dispositivo (#3), evidencia física (#5) y plan de credenciales
   de producción separado del laboratorio.

## Alcance y límites de la verificación

Revisión de lectura sobre `656d31a`: se recorrieron ambos backends completos
(rutas, dominio, puente MQTT, migraciones, seed), simulador, scripts de
escenario, Compose, CI, protocolo y documentación; las suites citadas se
ejecutaron durante la Fase 3.6 (resultados en el Plan Maestro). No se auditó a
profundidad la lógica de componentes de `apps/web/src` (solo sus flujos de
carga y el filtro de historial), el historial Git completo, ni se realizaron
mediciones de carga o pruebas dinámicas de seguridad. No se validó hardware
físico. Esta revisión no modificó código; solo se actualiza este resumen.
