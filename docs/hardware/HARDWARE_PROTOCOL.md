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
