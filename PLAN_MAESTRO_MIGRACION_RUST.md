# Plan maestro de migración a Rust — LOCKER

LOCKER sigue siendo un laboratorio aislado de préstamos, retiros, devoluciones y custodia de herramientas. El objetivo es migrar el backend a Rust, mantener y mejorar sus funciones, y optimizar recursos, conservando PostgreSQL, MQTT y, inicialmente, React/TypeScript. El diseño objetivo es un monolito modular; Axum, Tokio y SQLx son candidatos prioritarios.

## Condiciones del laboratorio

- Mantener `admin` / `admin` funcionando en todas las fases del laboratorio. No exigir MFA, OAuth, LDAP, rotación, recuperación de contraseñas ni sistemas avanzados de sesiones durante la migración simulada.
- Estas credenciales no son seguras para producción. No exponer el laboratorio públicamente ni conectar activos reales o controladores físicos mientras persistan sus riesgos.
- Autenticación insuficiente, contraseñas en texto claro y expuestas, MQTT anónimo y ausencia de TLS y autorización por tópico son **riesgos P0 aceptados temporalmente en laboratorio**; no se consideran resueltos. Los detalles y demás hallazgos se conservan en [RESUMEN_AUDITORIA.md](RESUMEN_AUDITORIA.md).
- La fase 5 es requisito antes de cualquier exposición pública, control físico o uso con activos reales. No bloquea el desarrollo con simulación aislada.
- Transacciones, idempotencia, consistencia de estados y prevención de duplicados son requisitos funcionales de la migración. Una apertura simulada no demuestra por sí sola el movimiento o cierre físico.
- Ningún par de componentes puede procesar simultáneamente la misma operación: durante el cambio, detener solicitudes, conciliar operaciones inciertas, desconectar al consumidor anterior y verificarlo antes de habilitar el nuevo. Aplicar el mismo criterio al revertir; no reenviar aperturas inciertas automáticamente.

## Fases y criterios

| Fase | Alcance y dependencias | Criterios de aceptación |
|---|---|---|
| **0 — Laboratorio y caracterización** | Seed explícito, arranque sin seed, PostgreSQL y MQTT aislados, pruebas de préstamo, devolución y reinicio de proceso. | Reinicio real conserva los datos simulados y no duplica registros; seed no automático; pruebas, Ruff, mypy y frontend pasan; plan actualizado. No se exige inspeccionar bases históricas, copia ni restauración. |
| **1 — Infraestructura Rust** | Tras fase 0, crear `apps/api-rust`, configuración, salud, compilación y CI, con Axum/Tokio/SQLx. Python sigue atendiendo el laboratorio. | Rust arranca aisladamente; pruebas y compilación pasan; datos y contratos existentes no cambian. |
| **2 — Dominio funcional** | Migrar administración, inventario, autorizaciones, préstamos y devoluciones por módulos; conservar PostgreSQL y los datos. Mantener acceso `admin` / `admin` de laboratorio. | Pruebas de paridad funcional, transacciones, concurrencia, reservas e idempotencia; sin préstamos ni ubicaciones duplicados. |
| **3 — MQTT, simulador y operaciones** | Migrar el puente y las operaciones simuladas; conservar MQTT, correlación y un único propietario activo. Incorporar recuperación y consistencia de estados. | Simulador completa flujos; probar desconexión, duplicados, mensajes tardíos, transferencia y reversión sin dos consumidores de la misma operación. |
| **4 — Frontend, optimización y retirada de Python** | Mantener React/TypeScript, corregir el historial y reducir cargas innecesarias; medir recursos y latencia con la misma carga. Retirar Python solo tras verificar paridad. | Flujos simulados completos, pruebas y CI pasan; comparación medida; procedimiento de reversión compatible con los datos. |
| **5 — Seguridad y preparación futura** | Revisar autenticación, contraseñas, autorización, TLS, MQTT por dispositivo, auditoría y protocolos físicos antes de producción. Definir un plan de credenciales de producción separado del laboratorio. | Análisis de amenazas y pruebas de acceso y dispositivos; validación física y operativa antes de activos reales o exposición pública. |

## Matriz de hallazgos de auditoría

| Nº | Tratamiento | Fase |
|---:|---|---:|
| 1 | Autenticación ausente en rutas: riesgo temporal aceptado en laboratorio; endurecer antes de producción. | 5 |
| 2 | Contraseñas en texto claro y expuestas: mantener credenciales de laboratorio; corregir para producción. | 5 |
| 3 | MQTT anónimo y sin identidad verificada: mantener aislado; asegurar antes de hardware real. | 5 |
| 4 | Seed automático: corregir y probar reinicio. | 0 |
| 5 | Apertura no prueba entrega: mantener distinción y caracterizar simulación; evidencia física antes de activos reales. | 2–3, 5 |
| 6 | Envío no durable: transacción, recuperación y pruebas de fallos. | 2–3 |
| 7 | Retiros concurrentes: transacciones e idempotencia. | 2 |
| 8 | Jerarquía y ocupación: validar relaciones y reservar compartimiento. | 2 |
| 9 | Identidad de locker ambigua: identificador global en protocolo simulado. | 3 |
| 10 | Actor de auditoría ausente: conservar trazabilidad funcional; atribución confiable en endurecimiento. | 2, 5 |
| 11 | CRUD sin validación: validar contratos y estados durante migración. | 2 |
| 12 | Identidad MQTT compartida: un solo procesador y transferencia controlada. | 3 |
| 13 | Consultas completas: paginar y cargar por vista tras medición. | 4 |
| 14 | Filtro de historial incorrecto: corregir y probar con dos activos. | 4 |
| 15 | Presencia sin evidencia: procesar latidos y mostrar estado real. | 3–4 |
| 16 | Falta de pruebas: caracterización y regresión por fase. | 0–5 |
| 17 | Fallos sin recuperación: estados y conciliación. | 2–3 |
| 18 | Esquemas sin uso: reutilizar lo necesario y retirar el resto con Python. | 2, 4 |

## Validaciones y límites

Las pruebas de Fase 0 utilizan exclusivamente datos y servicios aislados. La inspección de bases LOCKER anteriores, las copias y su restauración están fuera del alcance actual y no bloquean el desarrollo simulado. No modificar automáticamente datos existentes. La preparación de una eventual migración de datos reales se planificará antes de usar dichos datos.

En cada fase conservar contratos y datos funcionales, probar fallos y medir CPU, memoria, latencia y red antes de afirmar mejoras. Una futura PWA consumirá los mismos servicios; no forma parte de la fase 0.

## Avance de Fase 1

`apps/api-rust` contiene configuración, pool PostgreSQL y endpoints de salud con
Axum/Tokio/SQLx; no implementa préstamos ni MQTT. El perfil Docker opcional usa
un PostgreSQL distinto, temporal y sin migraciones ni seed. Python mantiene las
operaciones simuladas. Se validaron conexión disponible e indisponible, parada
ordenada y funcionamiento independiente del laboratorio. El acceso LAN desde
otro equipo no se ha validado y permanece deshabilitado por defecto.

### Cierre definitivo de Fase 1

- El binario Rust escucha por defecto en `127.0.0.1:8001`; Compose configura
  `0.0.0.0:8001` dentro del contenedor y publica `127.0.0.1:18001` en el host.
  Se comprobó el binario directamente en WSL y el servicio Docker en ejecución.
- CI construye la imagen Rust opcional y ejecuta `scripts/test_rust_docker.py`
  con PostgreSQL y red temporales: comprueba el bind, salud 200, disponibilidad
  503 sin PostgreSQL y parada ordenada. La imagen PostgreSQL es oficial.
- `RUST_DB_TIMEOUT_SECONDS` admite de 1 a 30 segundos; el healthcheck Docker
  dispone de 40 segundos, con al menos diez segundos de margen.

Validación local: Cargo fmt, clippy, pruebas y compilación release; imagen y
salud Docker Rust; pruebas de API, integración y simulador; Ruff, mypy, lint,
prueba y build web; configuración Compose y `git diff --check`. El frontend
en `127.0.0.1:8083` y la salud Rust en `127.0.0.1:18001` respondieron 200.
El workflow actualizado requiere una ejecución posterior en GitHub para confirmar
el resultado remoto; no se ha iniciado la Fase 2.

## Avance de Fase 2

`apps/api-rust` implementa el dominio funcional completo bajo `/api/v1`
(Axum/SQLx/PostgreSQL): autenticación, CRUD administrativo validado para las
9 entidades, ubicaciones con jerarquía verificada, historial, detalle de locker,
autorizaciones con transiciones, retiro, préstamos, devolución, operaciones,
auditoría con actor y dashboard. El puente MQTT de aperturas y su confirmación
siguen en Fase 3: una operación solicitada queda registrada como `PENDING`, sin
declarar éxito físico.

Hallazgos resueltos en Rust dentro de esta fase: **#7** (bloqueo `FOR UPDATE`
sobre autorización y herramienta, único préstamo activo por herramienta, prueba
concurrente con un solo préstamo), **#8** (jerarquía sucursal–locker–compartimiento
validada en la ubicación, compartimento reservado u ocupado rechazado antes de
ordenar la devolución, y bloqueo `FOR UPDATE` de la fila del compartimento para
que devoluciones y ubicaciones concurrentes al mismo destino se serialicen), **#10** (todo evento de auditoría registra `actor_id`),
**#11** (contratos CRUD validados: campos requeridos, tipos, longitudes, UUID,
FK inexistente → 422, duplicado → 409), **#17 parcial** (transiciones y fallos
explícitos sin estado parcial; la conciliación de `PENDING` depende del puente
de Fase 3), **#6 parcial** (operación y préstamo transaccionales, reintentos
idempotentes devuelven la misma operación; la entrega MQTT durable es Fase 3),
**#18 parcial** (no se eliminaron esquemas sin uso; se retiran con Python en
Fase 4). #5 se mantiene por diseño: una apertura simulada no demuestra movimiento.

Desviaciones deliberadas del contrato Python, exigidas por esta fase:
`password` nunca se serializa en respuestas; un reintento de retiro o devolución
devuelve la operación existente (202) en lugar de 409; los errores de validación
responden 422/409 con `detail` en lugar del 500 de Python. `scripts/test_phase2_parity.py`
compara ambos backends contra la misma base aislada y verifica estas desviaciones.

Regla de consumidor único vigente: Python sigue atendiendo el laboratorio;
Rust se prueba contra bases aisladas. No conectar ambos a la misma base mutable.

Validación local ejecutada (19-11-2026, cierre de Fase 2): `cargo fmt --check`,
`cargo clippy --locked --all-targets -D warnings`, `cargo test --locked`
(6 unitarias) y `cargo test --locked -- --ignored` con PostgreSQL aislado
(8 de dominio — incluye una carrera de devoluciones al mismo compartimento —
más salud) pasaron; build release validado al construir la imagen Docker y
`scripts/test_rust_docker.py` pasó (bind loopback, 200/200, 503 sin PostgreSQL,
parada ordenada); la suite Python de integración aislada pasó (7 pruebas);
`scripts/test_phase2_parity.py` pasó 58 comparaciones contra la misma base
(listados en tres estados, login, flujo retiro/devolución, siete contratos de
error con mensajes idénticos, detalle de locker e historial); lint, build y
prueba web pasaron; `docker compose config` y `git diff --check` sin problemas.
La revisión de cierre corrigió además dos defectos de la primera iteración:
campos ausentes o no texto en login responden 422 (paridad con Python) y la
reserva de compartimento es a prueba de concurrencia. El workflow con el job
`parity` nuevo requiere una ejecución en GitHub para confirmar el resultado
remoto.

Riesgos pendientes de Fase 2: las operaciones quedan `PENDING` sin puente MQTT
(Fase 3); la conciliación de operaciones colgadas y la entrega durable (resto
de #6 y #17) requieren ese puente; la atribución de actor es la sesión admin
del laboratorio (sesiones reales en Fase 5); Python conserva su 500 en CRUD
inválido (solo Rust valida) hasta el cutover de Fase 4.

## Avance de Fase 3

`apps/api-rust/src/mqtt.rs` implementa el puente con rumqttc y QoS 1 sobre el
protocolo existente (mismos tópicos, envelope y correlación). La publicación
reclama operaciones `PENDING` de forma atómica (`UPDATE ... FOR UPDATE SKIP
LOCKED`) y marca `published_at` solo tras el PUBACK; la confirmación aplica
los efectos de dominio en una transacción espejo de `confirm()` de Python.
`/health/ready` exige la suscripción a `locker/+/events`, igual que Python.

Máquina de estados: `PENDING → PUBLISHING → PUBLISHED → CONFIRMED/FAILED`, con
`UNKNOWN` para lo indeterminado (PUBACK perdido por reinicio o desconexión, o
TTL de 60 s sin confirmación). `PUBACK` nunca significa apertura física (#5 se
mantiene). Al reiniciar: `PENDING` se republica (jamás llegó al broker);
`PUBLISHING` huérfana pasa a `UNKNOWN` y no se re-publica. Un evento tardío
confirma `PUBLISHED` o `UNKNOWN`; un PUBACK tardío nunca sobrescribe
`CONFIRMED` (corrección B). El TTL jamás revierte préstamos ni libera
reservas: lo incierto mantiene el bloqueo (corrección C).

Propiedad exclusiva (auditoría #12, corrección D): la columna `origin`
(migración `20261119_0004`, también reflejada en `schema.sql` y en el modelo
Python para paridad de respuestas) particiona el reclamo (Rust solo reclama
`origin='rust'`); la confirmación se particiona por estados reclamados (Python
solo confirma `PENDING`). `locker/+/events` compartido: cada puente ignora
correlaciones ajenas. El simulador deduplica comandos por `correlation_id`
(en memoria; un controlador físico futuro debe persistirla). Hallazgo #9
resuelto por verificación al publicar: código de locker repetido entre
sucursales ⇒ operación `FAILED` sin publicar.

Conciliación (corrección C): `POST /api/v1/operations/{id}/resolve` requiere
`Bearer lab-admin-token`, `outcome` (`CONFIRMED`/`FAILED`), `reason` y
`evidence`; audita con metadatos. `FAILED` revierte el préstamo (`CANCELLED`
con autorización reutilizable en checkout; `ACTIVE` en devolución). Un evento
tardío tras la resolución se ignora y se registra en el log.

Pruebas ejecutadas (19-11-2026, validación local): `cargo fmt --check`,
`cargo clippy --locked --all-targets -D warnings`, `cargo test --locked`
(6 unitarias), `cargo test --locked -- --ignored` con PostgreSQL y Mosquitto
reales (18 de dominio/MQTT más salud): flujo completo por broker, duplicado
sin doble efecto, evento antes de PUBACK sin sobreescritura, tardío tras
UNKNOWN, reinicio con `PENDING` republicada y huérfana a `UNKNOWN`, origen
Python nunca reclamado, código ambiguo sin publicación, conciliación con
guardas y reversión, readiness con gate MQTT. `scripts/test_rust_mqtt.py`
(contenedores desechables, simulador real): flujo completo, evento duplicado,
caída del broker con recuperación o `UNKNOWN` conciliable, reinicio real del
proceso a mitad de operación, y coexistencia Python/Rust sobre la misma base
con propiedad única verificada en ambas direcciones. `scripts/test_rust_docker.py`
ampliado: gate MQTT de readiness (503 sin broker, 200 al volver).
`scripts/test_phase2_parity.py` con el puente Rust apagado (`RUST_MQTT_ENABLED=false`)
pasó 58 comparaciones con las columnas nuevas idénticas en ambos backends.
Suite Python aislada 8/8 (incluye la prueba de migración 0004: base 0003 con
datos, upgrade preserva, downgrade limpio); simulador 5/5 (incluye dedup);
Ruff y mypy de la API, Ruff del simulador, `docker compose config` y
`docker compose --profile rust config` sin problemas. La imagen Rust se
construyó varias veces (equivalente a `cargo build --release`).

Desviaciones deliberadas del contrato, exigidas por esta fase: el endpoint
`resolve` exige `Bearer lab-admin-token` (ningún otro endpoint lo exige aún,
hallazgo #1 diferido a Fase 5); el estado de préstamo `CANCELLED` es propio
de la reversión Rust (Python no lo crea); los estados de operación
`PUBLISHING`, `PUBLISHED` y `UNKNOWN` son nuevos y están documentados en el
protocolo. El workflow CI incorporó Mosquitto al job `rust`
(`LOCKER_TEST_MQTT_HOST`) y el job `mqtt` (`scripts/test_rust_mqtt.py`);
requiere una ejecución en GitHub para confirmar el resultado remoto. El primer
intento (run 16 sobre `656d31a`) falló en el job `rust`: los `services` se crean
antes de `actions/checkout` y no pueden montar `infra/docker/mosquitto.conf`;
el broker se inicia ahora con `docker run` tras el checkout (montaje explícito
de solo lectura, puerto en `127.0.0.1:1883`, espera de conexión MQTT y limpieza
con `if: always()`), manteniendo el servicio PostgreSQL y los comandos de Cargo
intactos.

## Fase 3.6 — Estabilización (cierre técnico)

Revisión de la Fase 3 (hallazgo Must Fix + 6 menores, corregidos y verificados):

- **`reason` de conciliación acotado a 255 caracteres** (límite real de
  `locker_operations.failure_reason`, contado en caracteres igual que
  PostgreSQL): el exceso responde 422 con mensaje de contrato en lugar del 500
  de base de datos. Pruebas de límite exacto (pasa validación, rechazado solo
  por estado 409) y de un carácter en exceso (422).
- **`resolve` con `outcome: CONFIRMED` cuyas precondiciones fallan** (p. ej.
  compartimento de devolución ocupado): la operación termina `FAILED` con su
  `failure_reason`, el préstamo pendiente se revierte (nuevo helper compartido
  `revert_pending_loan`, también usado por el veredicto `FAILED`), la
  auditoría registra veredicto y estado resultante, y la respuesta expone
  `status` y `failure_reason`. Prueba de regresión dedicada. El panel
  administrativo debe mostrar esos campos (registrado para Fase 4).
- **Prueba de migraciones**: el `finally` ya no enmascara errores (el engine se
  crea antes del primer upgrade, conexión perezosa) y una prueba nueva fija
  que un fallo de Alembic nunca se reporta como `NameError` del cleanup.
- **Limpieza**: `command_envelope` sin referencias eliminado (la única copia
  del envelope es la del publicador); la aserción débil del escenario Docker
  se reemplazó por la verificación real "toda resolución manual queda en
  auditoría con su motivo" (rama por rama).
- **Barrido TTL cada 5 s** en lugar de cada 250 ms: una operación vencida
  pasa a `UNKNOWN` dentro de TTL + 5 s (documentado en el protocolo), sin
  escritura periódica innecesaria contra la base.
- **Dedup del simulador acotado** a los últimos 10 000 `correlation_id`
  (deque + set): dentro de la ventana deduplica, fuera de ella o tras
  reinicio responde de nuevo. Documentado explícitamente como caché acotada,
  no como garantía de idempotencia persistente (requisito para hardware
  real). Pruebas de ventana, expiración y pérdida por reinicio.

Validación local ejecutada (19-11-2026): `cargo fmt --check`,
`cargo clippy --locked --all-targets -D warnings`, `cargo test --locked`
(6 unitarias), `cargo test --locked -- --ignored --test-threads=1` con
PostgreSQL y Mosquitto reales: **19/19** (dominio + MQTT, incluida la
regresión nueva de conciliación). `scripts/test_rust_mqtt.py` completo
(flujos por broker, duplicado, caída/reinicio de broker, reinicio real del
proceso, coexistencia Python/Rust); `scripts/test_rust_docker.py` (gate
MQTT). Suite Python aislada **9/9** (migración 0004 con datos preservados,
downgrade limpio y error no enmascarado); simulador Ruff + **7/7** (ventana
de dedup); Ruff y mypy de la API; paridad Python/Rust (58 comparaciones,
puente Rust apagado); lint, prueba y build web en Node 22; `docker compose
config` y `--profile rust`, `git diff --check` sin problemas. Todas PASSED.

Sin cambios de contrato REST nuevos en 3.6; el comportamiento de `resolve`
documentado en `docs/hardware/HARDWARE_PROTOCOL.md` es el implementado.
Queda pendiente la validación CI remota sobre el commit de cierre (jobs
`rust` con Mosquitto, `mqtt` nuevo, integración, paridad, compose y web).

Riesgos pendientes de Fase 3 (sin cambios): UNKNOWN conservador tras caída de
broker; perfil `rust` de laboratorio sin esquema automático; auth solo en
`resolve` (#1 → Fase 5); dedup persistente exigible a hardware real; MQTT
anónimo sin TLS (#3 → Fase 5); latidos `locker/+/status` (#15) y la
visualización de `resolve` en el panel quedan para Fase 4.

### Validación CI remota — FASE 3 VALIDADA EN CI

- **Commit:** `37026b02aee1c23e2e5ea19d49c3ab604627b34d` (contiene la Fase 3
  completa, la estabilización 3.6, la auditoría actualizada y la corrección
  del job `rust`).
- **Workflow:** run #18, 19-11-2026, `conclusion: success` —
  https://github.com/mateoschreiber/locker/actions/runs/44599990488
- **Jobs 8/8 aprobados:** `rust` (broker iniciado con `docker run` tras el
  checkout; fmt, clippy, pruebas unitarias, integración con PostgreSQL y
  Mosquitto, build release), `api`, `simulator`, `web`, `compose`
  (incluye smoke Docker Rust con gate MQTT), `integration` (suite Python
  aislada y escenario de reinicio), `parity` (contratos Python/Rust) y `mqtt`
  (escenario completo: flujos por broker, duplicado, caída de broker,
  reinicio real de proceso y coexistencia con propiedad única).
- **Antecedente:** el run #16 sobre `656d31a` falló en `rust` por el montaje
  del servicio Mosquitto antes del checkout; corregido en `37026b0`
  (documentado arriba).

Con esta ejecución, la Fase 3 queda completada localmente, validada
localmente y **validada en CI**. Los riesgos pendientes y el alcance de las
Fases 4 y 5 se conservan sin cambios: no se ha iniciado el cutover, Python
sigue siendo el backend activo del laboratorio y no se ha conectado hardware
real.


## Avance de Fase 4.0 — contratos e integridad previos al cutover

F4.0 corrige los defectos contractuales y de integridad pendientes antes del
cutover. Python sigue atendiendo el laboratorio; nada de este bloque cambia
qué backend está conectado ni inicia F4.1/F4.2.

- **#19 cerrado:** matriz explícita de estados administrativos por entidad
  (`ACTIVE/INACTIVE`, `ONLINE`, `LOCKED`, `SIMULATED`, `AVAILABLE/INACTIVE`;
  `ONLINE` es etiqueta administrativa, no presencia MQTT — eso llega en F4.1),
  validada en Python y Rust con `422` idéntico y paridad verificada. Los
  estados de dominio (`ON_LOAN` de herramientas; préstamos, autorizaciones y
  operaciones no tienen rutas de escritura) no son editables. Guardias `409`
  con bloqueo transaccional (`FOR UPDATE`, el mismo que checkout/devolución)
  para herramientas con préstamo activo o pendiente y compartimentos con
  ubicación activa o devolución pendiente; `deactivate` ya no permite eludir
  las guardias de PATCH. El panel sustituyó el `prompt()` por una lista por
  entidad y no ofrece controles sobre estados de dominio.
- **#2 (dump) cerrado:** `dump()` de Python ya no expone `password` en ninguna
  respuesta (login, `/auth/me`, listados, detalle, CRUD), con prueba en la
  suite aislada. El almacenamiento en claro y HTTPS siguen siendo Fase 5.
- **#14 cerrado:** el historial de inventario consulta
  `GET /tools/{id}/history` por selección con `AbortController`; una respuesta
  tardía de otra herramienta no puede sobrescribir la visible (prueba con el
  orden de respuestas invertido), muestra la ubicación activa y estados
  vacíos explícitos.
- **Integridad MQTT (hallazgos #22/#23 de la revisión de F4.0):** los fallos
  con resultado físico **conocido como no ejecutado** (publicación que nunca
  salió, p. ej. código de locker ambiguo) revierten préstamo y autorización en
  la misma transacción con el helper compartido `revert_pending_loan`. Los
  fallos **inciertos** en confirmación por evento (compartimento ocupado,
  ubicación desaparecida, datos que no coinciden) nunca liberan recursos
  automáticamente: la operación queda `FAILED` reteniendo el préstamo y
  `resolve` la acepta mientras dure esa retención, con atestación auditable y
  transición terminal idempotente (un evento tardío o una segunda resolución
  no cambian nada). La reserva de compartimentos de `create_placement` cubre
  `PENDING/PUBLISHING/PUBLISHED/UNKNOWN` (antes solo `PENDING`, la ventana que
  el puente abre en ~250 ms), y la confirmación MQTT bloquea la fila del
  compartimento como los escritores REST.
- **Pruebas frontend reales** (Vitest + Testing Library, dos dependencias de
  desarrollo): historial aislado e invertido, edición de estado con PATCH,
  mensajes de error del backend y estados vacíos.
- **Línea base de rendimiento** registrada en
  [`docs/PERFORMANCE_BASELINE.md`](docs/PERFORMANCE_BASELINE.md) para comparar
  F4.2 (cálculo del ciclo, latencias, tamaños, CPU/RAM observados).

Decisión sobre propiedad y conciliación (revisión de cierre de F4.0): el
`resolve` es deliberadamente transversal a `origin`. La propiedad exclusiva
(#12) rige el reclamo y la confirmación automáticos del puente (particionados
por `origin` y estados, sin cambios); la conciliación es una acción
administrativa autenticada, con evidencia exigida y auditoría con veredicto.
Restringirla por origen dejaría sin salida préstamos atascados del otro
backend en la base compartida de coexistencia (Python no tiene `resolve`).
Riesgo conocido y aceptado: un operador puede terminar una operación creada
por el otro backend; lo mitigan el token, la evidencia y la auditoría.

Corrección de cierre (mismo día, revisión final): el contrato de estado
completó los bordes que la primera iteración no cubría. `status: null` cuenta
como campo ausente en ambos backends (PATCH sin cambio involuntario, creación
con el valor por defecto — antes Python respondía 422); los valores que no
son texto responden `422 "Campo 'status': se requiere texto"` idéntico en
ambos (antes divergía el `detail`); ambos bordes tienen casos explícitos en la
paridad y en la suite aislada de Python. Sin cambios en Rust: su semántica
(null ausente, tipo texto) es la que Python ahora replica.

Validación local ejecutada (20-11-2026): `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`
(6 unitarias) y `cargo test --locked -- --ignored --test-threads=1` con
PostgreSQL y Mosquitto aislados: **26/26** (7 nuevas: contrato de estados y
guardias, fallo de publicación con reversión y reintento exitoso, devolución
con compartimento ocupado conciliable y evento tardío ignorado, checkout sin
ubicación conciliable, evento con datos incorrectos, reserva por cada estado
de devolución y concurrencia colocación/devolución al mismo compartimento);
build release equivalente al construir la imagen Docker;
`scripts/test_rust_docker.py` (bind loopback, gate MQTT, 503 sin PostgreSQL)
y `scripts/test_rust_mqtt.py` completos (flujo, duplicado, caída, reinicio,
coexistencia). Suite Python aislada **10/10** (nueva de contratos de estado,
exclusión de `password` y guardias) y `scripts/test_docker_restart.py`;
Ruff y mypy de la API. Paridad Python/Rust completa (puente Rust apagado),
incluyendo los **seis contratos nuevos** con `detail` idéntico (422 estado
desconocido, 422 estados administrativos de locker, 422 estado de dominio en
creación, 409 herramienta con préstamo, 409 desactivación de esa herramienta,
409 compartimento con ubicación). Lint, build y pruebas web (4 pruebas de
componentes) en Node 24 local; `docker compose config` y `git diff --check`
sin problemas.

Cambios de contrato deliberados de este bloque: PATCH/create con estado fuera
de la lista responden `422` (antes `200` en ambos backends: la prueba de CRUD
que fijaba el comportamiento antiguo se actualizó al contrato nuevo);
desactivar herramienta con préstamo o compartimento ocupado/reservado responde
`409` (antes `200`); `resolve` acepta `FAILED` con préstamo pendiente (antes
solo `UNKNOWN`); ninguna respuesta incluye `password` (Rust ya lo cumplía;
la paridad ahora compara el contrato limpio en ambos).

Riesgos pendientes conocidos (sin cambios): autenticación/token fijo y MQTT
anónimo sin TLS (#1/#3, Fase 5); almacenamiento de contraseñas en claro (#2
resto, Fase 5); latidos #15, panel de conciliación/estados de operación y
paginación #13 (F4.1/F4.2); esquemas sin uso #18 (se retiran con Python).
No se realizó el cutover: Python sigue conectado al laboratorio y Rust
permanece aislado.

## Avance de Fase 4.1 — A: presencia observada (MQTT)

- El puente Rust consume `locker/+/status` en la misma conexión, suscrita
  **solo tras** el SUBACK de `locker/+/events`: el readiness sigue exigiendo
  la suscripción de eventos y una suscripción de status jamás la satisface.
- La presencia es estado observacional **en memoria** (sin migración ni
  escrituras): se recupera de reinicios del backend por el replay de mensajes
  retained del broker, revalidando timestamps; un retained antiguo nunca
  presenta un locker como conectado.
- Estados derivados en lectura (sin barridos): `ONLINE` (comunicación
  reciente y válida), `OFFLINE` (Last Will o mensaje explícito, persistente
  hasta nueva evidencia), `STALE` (evidencia vencida;
  `RUST_PRESENCE_STALE_SECONDS`, 90 s por defecto, 30–600) y `UNKNOWN` (sin
  datos o código duplicado entre sucursales, `ambiguous_code: true`: la
  presencia no se atribuye al dispositivo equivocado, hallazgo #9).
  Mensajes malformados, tipos desconocidos y timestamps futuros (>30 s de
  skew) se ignoran; los heartbeats no generan auditoría.
- Contratos nuevos solo-Rust, declarados por `GET /api/v1/capabilities`:
  `GET /api/v1/lockers/presence` (identificador, código, nombre, estado
  administrativo, presencia, última comunicación, antigüedad, origen y
  actualización; sin datos no inventa disponibilidad) y el propio
  `capabilities`. Python responde 404: el panel no envía esas solicitudes al
  backend activo del laboratorio.
- Panel: vista Lockers con badge de presencia y última comunicación
  (columnas solo presentes con datos), detalle con presencia observada y
  aviso de código duplicado; el dashboard deja de afirmar "Todo listo para
  operar" mientras exista una operación por conciliar o un locker
  administrativamente activo sin presencia confirmada.

Validación local ejecutada (20-11-2026): `cargo fmt --check`,
`clippy --locked --all-targets -- -D warnings`, unitarias **8/8** (2 nuevas de
presencia: ingest y derivación de estados) e integración aislada con
PostgreSQL y Mosquitto **27/27** (nueva `presence_lifecycle_and_recovery_over_broker`:
sin datos → UNKNOWN, retained antiguo → STALE, heartbeat fresco → ONLINE con
antigüedad y origen, OFFLINE persistente, mensaje malformado ignorado,
código duplicado no atribuido, código inexistente no listado y reinicio del
backend con store nuevo recuperando por replay retained). Lint, build y
pruebas web (7 de componentes: presencia por locker con y sin capacidades,
dashboard con intervención pendiente). La paridad no cambia sus contratos
compartidos; la validación completa de F4.1 se ejecuta al cierre del bloque C.

## Avance de Fase 4.1 — B: panel de operaciones y conciliación

- Nueva vista **Operaciones**: estado, tipo, herramienta, locker y
  compartimento, usuario, correlación, origen y fechas de solicitud,
  publicación y confirmación con filtro por estado y detalle expandible. Los
  identificadores legibles (código/nombre) reemplazan a los UUID como
  identificador principal; los técnicos quedan accesibles en el detalle. Las
  operaciones que requieren intervención (UNKNOWN, o FAILED con préstamo
  pendiente) se marcan y ordenan primero, y el dashboard las refleja junto a
  la presencia.
- Conciliación: reusa `POST /operations/{id}/resolve` **sin cambios de
  semántica** (restricción de F4.0 respetada). El formulario exige veredicto,
  motivo y evidencia con confirmación explícita y explica las consecuencias;
  la respuesta del backend es la autoridad final — un CONFIRMED que no puede
  aplicarse se informa con su estado resultante real (`status` +
  `failure_reason`). Tras resolver se refrescan operaciones, préstamos y
  auditoría, y una segunda resolución terminal se rechaza del lado del
  backend y se muestra comprensible.
- Token de laboratorio: el panel guarda el `access_token` de la respuesta de
  login en memoria (ninguna constante nueva) y lo envía únicamente en la
  solicitud de conciliación.
- Compatibilidad: el formulario y el badge de intervención solo se ofrecen
  cuando `GET /api/v1/capabilities` declara `resolve` (Python responde 404;
  el panel nunca le envía esa solicitud), con prueba explícita de que la
  solicitud no se emite.

Validación local ejecutada (20-11-2026): lint, build y pruebas web
**11/11** (4 nuevas de F4.1-B: enriquecimiento con nombres/códigos y orden por
intervención, filtro por estado, elegibilidad UNKNOWN/FAILED-con-préstamo vs
CONFIRMED, veredicto CONFIRMED con respuesta FAILED real y header
`Authorization`, rechazo 409 comprensible y backend sin capacidad que jamás
recibe la solicitud). El backend no cambió: `resolve`, elegibilidad y
`capabilities` ya estaban validados por las 27 pruebas de integración de
F4.0/F4.1-A.

## Avance de Fase 4.1 — C: entorno aislado, pruebas y estabilización

- **Entorno reproducible** `compose.rust-preview.yaml`: PostgreSQL, Mosquitto,
  simulador, `api-rust` y `web-rust` en proyecto, red, broker y volúmenes
  desechables propios (nada compartido con el laboratorio; el broker aislado
  impide que un comando llegue al simulador activo). El esquema y el seed LAB
  se inicializan explícitamente con la imagen Python (`alembic upgrade head` +
  seed que rechaza base no vacía) y el nginx del preview
  (`apps/web/nginx/rust.conf`) apunta a Rust **sin modificar la configuración
  productiva**. TTL corto (5 s) para escenarios inciertos rápidos.
- **Smoke reproducible** `scripts/test_rust_preview.py`, todo a través de
  nginx: `/health/ready` 200, `capabilities`, login, presencia ONLINE por
  latido del simulador real, flujo completo checkout/return confirmado,
  pérdida del simulador (Last Will → presencia OFFLINE), **reinicio real del
  broker** con la presencia restaurada por el replay retained persistido,
  UNKNOWN por TTL conciliado como CONFIRMED con préstamo activo y
  **autorización verificada CONSUMED** (solo una reversión la devuelve a
  APPROVED), **reinicio real de api-rust con operación en vuelo** → UNKNOWN →
  resolución CONFIRMADA con efectos de dominio y registro de auditoría.
  Incorporado al job `compose` de CI.
- **Rendimiento** (ver delta en `docs/PERFORMANCE_BASELINE.md`): el ciclo
  contra Python queda en 15 solicitudes (las capacidades se detectan una vez y
  el panel no emite presencia ni resolve); contra Rust el ciclo es 16 — el
  endpoint de presencia mide 6,4 ms promedio / 336 B (estado en memoria) — y
  la vista de Operaciones no añade solicitudes.

Validación local ejecutada (20-11-2026, cierre de F4.1 y revisión técnica
final): smoke de preview completo PASSED dos corridas consecutivas (escenarios
arriba, incluidos los añadidos en la revisión final: autorización CONSUMED
tras conciliar CHECKOUT UNKNOWN como CONFIRMED, y recuperación de presencia
tras reinicio real del broker por retained persistido); la regresión Rust
`resolve_confirmed_checkout_keeps_the_authorization_consumed` (loan ACTIVE,
tool ON_LOAN, autorización CONSUMED, ubicación retirada) eleva la integración
a **28/28** con fmt/clippy limpios; `scripts/test_rust_mqtt.py` completo
(flujo, duplicado, caída de broker, reinicio, coexistencia con propiedad
única) y `scripts/test_rust_docker.py` (gate MQTT, 503 sin PostgreSQL) sobre
imágenes reconstruidas; paridad Python/Rust completa OK (contratos compartidos
sin cambios); `docker compose config` (laboratorio, perfil `rust` y preview)
sin problemas; lint, build y pruebas web **11/11**; `git diff --check`
limpio. Sin cutover: Python sigue atendiendo el laboratorio.

## Cierre de los tres hallazgos de Fase 0



1. **Exposición del panel:** Compose publica `127.0.0.1:8083` por defecto. `LOCKER_BIND_ADDRESS` permite seleccionar explícitamente una dirección privada; la configuración efectiva se comprobó tanto con el valor predeterminado como con una dirección LAN de ejemplo. El acceso LAN requiere restringir la red y no convierte las credenciales de laboratorio en seguras.
2. **Sincronización MQTT:** la API marca la suscripción activa al recibir SUBACK y `/health/ready` responde 503 hasta entonces. El simulador anuncia disponibilidad después del SUBACK y usa aviso retenido de desconexión; las pruebas esperan una señal reciente con timeout y diagnóstico. Se probaron aceptación, rechazo y desconexión de suscripciones.
3. **Reinicio real Docker:** `python scripts/test_docker_restart.py` crea un proyecto Compose aislado con el Dockerfile normal, verifica arranque sin seed y login después del seed explícito, crea un préstamo, reinicia el contenedor y compara exactamente préstamos, ubicaciones y conteos de todas las tablas. Después ejecuta devolución, nuevo retiro y devolución. Sus recursos temporales se eliminan al terminar.

Validación ejecutada: integración API/PostgreSQL/MQTT 7 pruebas; simulador 4 pruebas; escenario de reinicio Docker aprobado; Ruff y mypy de API, Ruff de simulador, lint/build/test web (1 prueba), comprobación de Compose y `git diff --check` satisfactorios. La integración incluye avisos de deprecación existentes de FastAPI/Starlette. No se han probado hardware ni datos históricos.
