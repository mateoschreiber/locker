# Protocolo MQTT v1

Tópicos por locker:

- `locker/{locker_id}/commands`
- `locker/{locker_id}/events`
- `locker/{locker_id}/status`

Todo mensaje JSON incluye `message_id`, `correlation_id`, `protocol_version`,
`locker_id`, `compartment_id` opcional, `timestamp` UTC, `type` y `payload`.

La Fase 0 implementa en `status` los eventos retained `LOCKER_ONLINE` y
`LOCKER_HEARTBEAT`; el simulador se suscribe a `commands` y registra mensajes
de diagnóstico. Eventos físicos (puerta, RFID y cerradura) comenzarán en la
Fase 3 y deberán reutilizar este envelope.

## Operación simulada (Fase 2)

La API publica `OPEN_COMPARTMENT` en `commands` con el UUID del compartimiento
en `compartment_id`. El simulador responde `COMPARTMENT_OPENED` en `events`,
conservando el mismo `correlation_id`. La API solo confirma retiro o devolución
cuando recibe este evento correlacionado.

## Ciclo de vida de una operación (Fase 3)

El puente Rust (`apps/api-rust/src/mqtt.rs`) publica con QoS 1 y confirma
eventos correlacionados. La columna `origin` de `locker_operations` particiona
la propiedad: el puente Rust solo reclama filas `origin='rust'` y solo
confirma sus estados reclamados (`PUBLISHING`, `PUBLISHED`, `UNKNOWN`); las
filas `PENDING` de origen Python las confirma exclusivamente el puente Python.
Ningún par de componentes procesa la misma operación, incluso contra una base
compartida (hallazgo #12).

Estados de `locker_operations.status`:

```text
PENDING ──reclamo atómico──▶ PUBLISHING ──PUBACK──▶ PUBLISHED ──evento──▶ CONFIRMED
   │                             │                    │  └─datos no coinciden─▶ FAILED
   │                             │                    └─TTL sin evento─▶ UNKNOWN
   │                             └─PUBACK indeterminable (reinicio/desconexión)─▶ UNKNOWN
   └─reinicio: se republica (nunca llegó al broker)
```

Reglas:

- **PUBACK ≠ apertura física.** `published_at` registra que el broker aceptó el
  comando; solo `COMPARTMENT_OPENED` (o una conciliación con evidencia)
  confirma la ejecución física (hallazgo #5).
- **Nunca se republica lo incierto.** Al reiniciar, `PENDING` se publica (el
  comando jamás llegó al broker); `PUBLISHING` huérfana pasa a `UNKNOWN` y no
  se reabre automáticamente: no puede determinarse si el compartimento ya se
  abrió antes.
- **Eventos duplicados** (QoS 1 permite reentrega) se ignoran: la
  transición solo aplica una vez por `correlation_id` mediante `UPDATE`
  condicional. El simulador deduplica comandos por `correlation_id` en una
  ventana acotada en memoria (los últimos 10 000, perdida al reiniciar): un
  duplicado dentro de la ventana se ignora; uno expirado de la ventana o
  previo a un reinicio se responde de nuevo. Esta caché **no es una garantía
  de idempotencia persistente**: un controlador físico futuro debe persistir
  la deduplicación entre reinicios y rechazar `correlation_id` desconocidos.
- **Eventos fuera de orden**: `COMPARTMENT_OPENED` puede llegar antes del
  PUBACK y confirma desde `PUBLISHING`; un PUBACK tardío nunca sobrescribe
  `CONFIRMED`/`FAILED` (actualización condicional).
- **Eventos tardíos** confirman también `UNKNOWN` si las precondiciones del
  dominio siguen vigentes.
- **TTL de confirmación** (`RUST_MQTT_CONFIRM_TIMEOUT_SECONDS`, 60 s por
  defecto): `PUBLISHED` sin evento pasa a `UNKNOWN` dentro de TTL + 5 s
  (cadencia del barrido). El timeout jamás revierte préstamos ni libera
  recursos: el desenlace físico incierto mantiene el bloqueo del préstamo y
  la reserva del compartimento.
- **Código de locker ambiguo**: el tópico usa el código del locker, único solo
  por sucursal; si dos lockers comparten código, la operación falla sin
  publicar (hallazgo #9).

## Conciliación de UNKNOWN

`POST /api/v1/operations/{id}/resolve` (Rust) requiere el token
`lab-admin-token`, un `outcome` (`CONFIRMED` o `FAILED`), un `reason` y una
`evidence`. Toda resolución queda en `audit_events` con su motivo y evidencia.

- `CONFIRMED`: el operador atestigua con evidencia que la apertura ocurrió; se
  aplican los mismos efectos de dominio que un evento MQTT. Si las
  precondiciones fallan (por ejemplo, el compartimento de la devolución quedó
  ocupado), la operación termina `FAILED` con su `failure_reason`, el préstamo
  pendiente se revierte y la respuesta expone `status` y `failure_reason`.
- `FAILED`: el operador atestigua que no ocurrió; el préstamo se revierte
  (`CANCELLED` en checkout, con la autorización vuelta a `APPROVED`; `ACTIVE`
  en devolución) y se libera la reserva.
- La auditoría registra el veredicto del operador y el estado resultante: un
  `CONFIRMED` que no pudo aplicarse queda auditado como
  `OPERATION_RESOLVED_CONFIRMED` con `status: FAILED` en sus metadatos.
- Un evento MQTT tardío tras la resolución se ignora y se registra en el log:
  el estado terminal y la atestación del operador prevalecen. El panel
  administrativo debe mostrar `status` y `failure_reason` de la respuesta
  (pendiente para la Fase 4).

Si no existe información suficiente, la operación permanece `UNKNOWN` con sus
recursos bloqueados y debe reportarse para conciliación operativa.

## Presencia observada (F4.1-A)

El puente Rust también consume `locker/+/status` (misma conexión, suscrita
solo tras el SUBACK de `locker/+/events`, de modo que el readiness sigue
exigiendo la suscripción de eventos). La presencia es estado observacional en
memoria del proceso Rust — sin escrituras a PostgreSQL — y se recupera tras un
reinicio del backend mediante el replay de mensajes retained del broker,
revalidando su timestamp.

Estados expuestos por `GET /api/v1/lockers/presence` (solo Rust):

- `ONLINE`: último mensaje válido con antigüedad ≤ umbral (90 s por defecto,
  `RUST_PRESENCE_STALE_SECONDS`, 30–600).
- `STALE`: existe evidencia pero vencida; **un retained antiguo nunca se
  presenta como conectado**.
- `OFFLINE`: `LOCKER_OFFLINE` (Last Will o explícito), persistente hasta
  nueva evidencia.
- `UNKNOWN`: sin información confiable, o código de locker duplicado entre
  sucursales (`ambiguous_code: true`) — la presencia no se atribuye al
  dispositivo equivocado (hallazgo #9).

Reglas: mensajes malformados, tipos desconocidos y timestamps futuros (más de
30 s de skew) se ignoran; códigos inexistentes no se listan; los heartbeats no
generan auditoría (evita crecimiento ilimitado de `audit_events`).

`GET /api/v1/capabilities` (solo Rust) declara `{resolve, presence}`: el panel
habilita conciliación y presencia únicamente cuando el backend conectado las
anuncia — el backend Python responde 404 y el panel nunca le envía esas
solicitudes.
