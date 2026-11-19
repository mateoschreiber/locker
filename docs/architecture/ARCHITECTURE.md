# Arquitectura

Locker usa un monolito modular: la API mantiene la lógica de dominio y el
estado relacional; las integraciones físicas se comunican mediante contratos
versionados de MQTT. La Fase 0 contiene solo conectividad y salud.

El navegador accede exclusivamente a Nginx. Nginx sirve la SPA y reenvía
`/api`, `/docs`, `/openapi.json` y `/health` a FastAPI. PostgreSQL y Mosquitto
solo existen en la red Docker `locker_internal`.

Las próximas fases crearán módulos de identidad, lockers, herramientas,
autorizaciones, préstamos y auditoría sin dividir el despliegue en
microservicios.

## Puente de operaciones (Fase 3)

Cada backend publica aperturas por MQTT y confirma el `COMPARTMENT_OPENED`
correlacionado en una única transacción con los efectos de dominio. La
propiedad de cada operación es exclusiva: la columna `origin` de
`locker_operations` particiona el reclamo de publicación (Rust solo reclama
`origin='rust'`) y los estados reclamados (`PUBLISHING`/`PUBLISHED`/`UNKNOWN`)
particionan la confirmación (Python solo confirma `PENDING`). Un
`PUBACK` del broker nunca significa apertura física; lo incierto pasa a
`UNKNOWN` y se resuelve con autorización administrativa, motivo y evidencia.
Los detalles de estados y conciliación están en
[HARDWARE_PROTOCOL.md](../hardware/HARDWARE_PROTOCOL.md).
