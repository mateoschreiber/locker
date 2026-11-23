# Protocolo MQTT v1

Tópicos por locker:

- `locker/{locker_id}/commands`
- `locker/{locker_id}/events`
- `locker/{locker_id}/status`

Todo mensaje JSON incluye `message_id`, `correlation_id`, `protocol_version`,
`locker_id`, `compartment_id` opcional, `timestamp` UTC, `type` y `payload`.

## Comandos y eventos de apertura

El backend (`apps/api-rust/src/mqtt.rs`) publica `OPEN_COMPARTMENT` con QoS 1 en
`locker/{code}/commands` con el UUID del compartimento en `compartment_id`. El
controlador (o simulador) responde `COMPARTMENT_OPENED` en
`locker/{code}/events` conservando el mismo `correlation_id`. El backend solo
confirma el retiro o la devolución cuando recibe este evento correlacionado.

## Ciclo de vida de una operación

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
  confirma la ejecución física.
- **Nunca se republica lo incierto.** Al reiniciar, `PENDING` se publica (el
  comando jamás llegó al broker); `PUBLISHING` huérfana pasa a `UNKNOWN` y no
  se reabre automáticamente.
- **Eventos duplicados** (QoS 1 permite reentrega) se ignoran mediante
  transición condicional en base de datos. El simulador deduplica comandos por
  `correlation_id` en una ventana acotada en memoria (últimos 10 000, perdida
  al reiniciar). Un controlador físico en producción debe persistir la
  deduplicación en almacenamiento no volátil y rechazar `correlation_id`
  desconocidos.
- **Eventos fuera de orden y tardíos:** `COMPARTMENT_OPENED` puede llegar antes
  del PUBACK y confirma desde `PUBLISHING`; un PUBACK tardío nunca sobrescribe
  `CONFIRMED`/`FAILED`. Un evento tardío también confirma `UNKNOWN` si las
  precondiciones del dominio siguen vigentes.
- **TTL de confirmación** (`RUST_MQTT_CONFIRM_TIMEOUT_SECONDS`, 60 s por
  defecto): `PUBLISHED` sin evento pasa a `UNKNOWN` dentro de TTL + 5 s. El
  timeout jamás revierte préstamos ni libera reservas de compartimento.
- **Código de locker ambiguo:** si dos lockers entre distintas sucursales
  comparten código, la operación falla sin publicar al broker.

## Conciliación administrativa (`UNKNOWN` / `FAILED`)

`POST /api/v1/operations/{id}/resolve` requiere autorización administrativa,
un `outcome` (`CONFIRMED` o `FAILED`), un `reason` (≤255 caracteres) y una
`evidence`. Toda resolución queda registrada en `audit_events`.

- `CONFIRMED`: aplica los efectos de dominio equivalentes al evento MQTT; si
  las precondiciones fallan (p. ej. compartimento ocupado), la operación
  termina `FAILED` con su `failure_reason` y libera el préstamo pendiente.
- `FAILED`: revierte el préstamo (`CANCELLED` en checkout devolviendo la
  autorización a `APPROVED`; `ACTIVE` en devolución) y libera la reserva.
- Un evento MQTT tardío posterior a la resolución se ignora.

## Presencia observada

El puente consume `locker/+/status` (suscrita tras el SUBACK de
`locker/+/events`) y mantiene la presencia en memoria del proceso Rust,
recuperándola tras reinicios mediante el replay de mensajes `retained` del
broker con validación de timestamp:

- `ONLINE`: último mensaje válido dentro de `RUST_PRESENCE_STALE_SECONDS`
  (90 s por defecto, rango 30–600).
- `STALE`: evidencia vencida (un `retained` antiguo nunca se reporta como
  conectado).
- `OFFLINE`: `LOCKER_OFFLINE` (Last Will o explícito), persistente hasta nueva
  evidencia.
- `UNKNOWN`: sin evidencia confiable o código duplicado entre sucursales
  (`ambiguous_code: true`).
