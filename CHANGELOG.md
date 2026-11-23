# Changelog & GitHub Releases

Todos los cambios notables del proyecto **LOCKER** se documentan en este archivo.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto adhiere a [Semantic Versioning](https://semver.org/lang/es/).

---

## [v0.5.0] — 2026-11-23 — Cierre de Migración a Rust (Fases 4.3 y 4.4)

### Added
- **Migración baseline SQLx (`apps/api-rust/migrations/20261123000001_baseline.sql`) y subcomando `locker-api-rust migrate`**: aplicación idempotente del esquema completo tanto en bases nuevas como sobre bases existentes creadas previamente con Alembic (`_sqlx_migrations`).
- **Inicialización nativa del laboratorio en Rust (`locker-api-rust seed`, `apps/api-rust/src/seed.rs`)**: reemplaza `python -m app.modules.seed`, exigiendo `APP_ENV=lab` y base de dominio vacía con bloqueo `ACCESS EXCLUSIVE`.
- **Prueba de integración `baseline_migration_and_lab_seed_populate_once_and_refuse_non_empty_db`** en `apps/api-rust/tests/api.rs` (34 pruebas de integración Rust).

### Changed
- **Cutover del laboratorio principal (`127.0.0.1:8083`, Fase 4.3)**: `compose.yaml` y `apps/web/nginx/default.conf` conectan el panel principal directamente a `api-rust` sobre el volumen persistente `locker_postgres_data`, habilitando presencia MQTT, paginación, operaciones enriquecidas y conciliación administrativa en el puerto `8083`.
- **Unificación de fuente de verdad del esquema**: `tests/api.rs`, `compose.yaml`, `compose.rust-preview.yaml` y todos los scripts (`rust-preview.ps1`, `test_rust_preview.py`, `bench_panel.py`, `test_rust_mqtt.py`, `check-local.ps1`) utilizan exclusivamente `locker-api-rust migrate` y `locker-api-rust seed`.

### Removed
- **Retirada definitiva del backend Python (Fase 4.4)**: eliminados `apps/api/` (FastAPI, SQLAlchemy, Alembic), `compose.test.yaml`, `scripts/test_phase2_parity.py`, `scripts/test_docker_restart.py` y `apps/api-rust/tests/schema.sql`. Python se conserva únicamente para el simulador de hardware (`apps/locker-simulator`) y scripts auxiliares de validación.

---

## [v0.4.2] — 2026-11-23 — Paginación Rust, Carga por Vista y Endurecimiento Ponytail

### Added
- **Paginación opt-in determinística en Rust (`/{entity}?limit=&offset=`)**: envoltorio `{ items, total, limit, offset }` para `limit` en `1..=200` y `offset >= 0` con orden `created_at DESC, id DESC`, manteniendo el array completo legado cuando `limit` no se especifica.
- **Listado enriquecido de operaciones (`GET /api/v1/operations?limit=`)**: una única consulta `JOIN` (sin N+1) incluye `tool_name`, `locker_code`, `compartment_code`, `user_display_name`, `loan_status` y `requires_intervention`, además del filtro `attention=1|0`.
- **Controles operativos completos en la vista `Operación` del panel React**: botones de acción directa en las tablas de **Autorizaciones** (`Aprobar`, `Retirar`, `Cancelar`) y **Préstamos** (`Devolver`).
- **Arnés de benchmark reproducible (`scripts/bench_panel.py`)**: medición automatizada sobre escenarios `1x`, `10x` y `100x` documentada en `docs/PERFORMANCE_BASELINE.md`.

### Changed
- **Sondeo del panel React por vista activa (`apps/web/src/polling.ts`)**: reducción de 16 solicitudes por ciclo (~245 req/min) a **≤3 solicitudes dinámicas por vista** (~45 req/min), suspensión automática en pestaña oculta y cancelación de peticiones obsoletas (`AbortController`), incluyendo paginación en `tools`, `operations`, `audit`, `authorizations` y `loans`.
- **Guardia de entidades inactivas en préstamos y retiros (Rust y Python)**: `POST /api/v1/authorizations` y `POST /api/v1/authorizations/{id}/checkout` rechazan operar sobre herramientas, usuarios, sucursales, lockers o compartimentos en estado `INACTIVE` con `409 Conflict` (`"No se puede operar con una entidad inactiva"`).
- **Optimización de `GET /api/v1/lockers/{id}/detail`**: ambos backends cargan únicamente las herramientas con ubicación activa en el locker consultado en lugar de recorrer toda la tabla `tools`.
- **Compuerta `schema_ready` en el puente MQTT de Rust (`apps/api-rust/src/mqtt.rs`)**: el reclamo periódico y el barrido TTL esperan a que `recover_orphans` confirme la existencia del esquema, eliminando el ruido de errores `42P01` en bases `tmpfs` vacías (cierra hallazgo #20).
- **Resolución DNS dinámica en `apps/web/nginx/default.conf`**: re-resolución de `api:8000` mediante el DNS embebido de Docker (`127.0.0.11`), evitando errores `502 Bad Gateway` tras reiniciar el contenedor del backend.

### Removed
- **Esquemas Pydantic sin uso en Python (`apps/api/app/modules/schemas.py`)**: eliminadas 12 clases huérfanas (cierra hallazgo #18).
- **Consultas SQL de reserva triplicadas en Rust**: unificadas en `operations::is_compartment_reserved`.

---

## [v0.4.1] — 2026-11-20 — Presencia MQTT Observada, Conciliación en Panel y Stack `rust-preview`

### Added
- **Presencia observada de lockers (`GET /api/v1/lockers/presence`)**: ingestión en memoria de `locker/{code}/status` (`LOCKER_ONLINE`, `LOCKER_HEARTBEAT`, `LOCKER_OFFLINE`) con estados derivados `ONLINE`, `STALE`, `OFFLINE` y `UNKNOWN` (con detección de `ambiguous_code` y recuperación por replay `retained`).
- **Detección explícita de capacidades (`GET /api/v1/capabilities`)**: permite al panel React habilitar presencia, conciliación, paginación y enriquecimiento contra Rust y degradar limpiamente contra Python.
- **Vista `Operaciones` y formulario de conciliación administrativa**: supervisión del ciclo completo y resolución auditada (`POST /api/v1/operations/{id}/resolve`) con veredicto, motivo y evidencia.
- **Entorno aislado `compose.rust-preview.yaml` y `scripts/rust-preview.ps1`**: stack completo en el puerto `8084` (y `8085` para smoke `scripts/test_rust_preview.py`) independiente del laboratorio principal `8083`.
- **Suite de validación local (`scripts/check-local.ps1`)**: modos `QUICK`, `FULL` y `E2E` ejecutables desde Windows PowerShell con contenedores Linux desechables y captura diagnóstica en `.local-validation/`.

---

## [v0.4.0] — 2026-11-20 — Contratos de Estado e Integridad Pre-Cutover

### Added
- **Matriz estricta de estados administrativos por entidad (Auditoría #19)**: validación idéntica `422 Unprocessable Entity` en Python y Rust; bloqueo de edición manual de estados de dominio (`ON_LOAN`).
- **Guardias transaccionales (`409 Conflict`)**: prohibición de desactivar o cambiar estado de herramientas con préstamo en curso o compartimentos ocupados/reservados por devoluciones (`PENDING`, `PUBLISHING`, `PUBLISHED`, `UNKNOWN`).
- **Conciliación de operaciones `FAILED` que retienen préstamo (Auditoría #22)**: `resolve` permite liberar recursos tras fallos en tiempo de evento con atestación del operador.

### Fixed
- **Exclusión total de `password` en respuestas REST (Auditoría #2-dump)**: `dump()` en Python y `dump_row()` en Rust jamás serializan la columna `password`.
- **Aislamiento del historial de herramienta en el panel (Auditoría #14)**: uso de `AbortController` en `/tools/{id}/history` para impedir que respuestas tardías sobrescriban la selección activa.

---

## [v0.3.0] — 2026-11-19 — Puente MQTT en Rust, Outbox Transaccional y Conciliación

### Added
- **Puente MQTT en Rust (`apps/api-rust/src/mqtt.rs`)**: publicación QoS 1 con reclamo atómico (`FOR UPDATE SKIP LOCKED`), máquina de estados `PENDING → PUBLISHING → PUBLISHED → CONFIRMED/FAILED` + `UNKNOWN`, y barrido TTL cada 5 s.
- **Propiedad exclusiva por operación (Auditoría #12)**: columna `origin` (`python` / `rust`, migración `20261119_0004`) y partición por estados reclamados, probada en coexistencia real (`scripts/test_rust_mqtt.py`).
- **Endpoint de conciliación (`POST /api/v1/operations/{id}/resolve`)**: resolución administrativa con token `Bearer lab-admin-token`, motivo acotado a 255 caracteres, evidencia y registro en `audit_events`.
- **Deduplicación acotada en el simulador (`apps/locker-simulator`)**: ventana en memoria de los últimos 10 000 `correlation_id`.

---

## [v0.2.0] — 2026-11-19 — Dominio Funcional Completo en Rust y Paridad REST

### Added
- **Migración completa del dominio a Rust bajo `/api/v1`**: autenticación, CRUD administrativo validado para las 9 entidades, ubicaciones con validación de jerarquía `sucursal → locker → compartimento`, autorizaciones, préstamos, devoluciones, operaciones, auditoría con actor y dashboard.
- **Concurrencia e idempotencia transaccional (Auditoría #6, #7, #8)**: bloqueos `FOR UPDATE` sobre autorizaciones, herramientas y compartimentos; reintentos idempotentes devuelven la operación existente (`202 Accepted`).
- **Arnés de paridad (`scripts/test_phase2_parity.py`)**: verificación automatizada de lecturas, escrituras y contratos de error entre Python y Rust contra una misma base aislada.

---

## [v0.1.0] — 2026-11-18 — Laboratorio Fase 0 e Infraestructura Rust Fase 1

### Added
- **Laboratorio aislado en Docker Compose**: PostgreSQL 17, Eclipse Mosquitto 2.0, FastAPI, simulador de locker y panel React/TypeScript tras Nginx en `127.0.0.1:8083`.
- **Inicialización explícita (`app.modules.seed`)**: carga del laboratorio `LAB` (24 compartimentos y 24 herramientas) protegida por `APP_ENV=lab` y verificación de base vacía.
- **Esqueleto del backend Rust (`apps/api-rust`)**: Axum + Tokio + SQLx con `/health/live`, `/health/ready` y perfil Docker opcional en `127.0.0.1:18001`.

---

## Cómo publicar un Release en GitHub (Sin GitHub Actions)

De acuerdo con las reglas del repositorio (`AGENTS.md`), no se ejecutan pushes automáticos ni workflows de publicación automática. Para crear y publicar un release en GitHub manualmente desde tu terminal local:

1. **Crear el tag anotado en `main`:**
   ```bash
   git tag -a v0.4.2 -m "Release v0.4.2: Paginación Rust, Carga por Vista y Endurecimiento Ponytail"
   ```
2. **Subir `main` y el tag a GitHub cuando lo decidas:**
   ```bash
   git push origin main --tags
   ```
3. **Crear el GitHub Release:**
   - **Opción A (GitHub CLI):**
     ```bash
     gh release create v0.4.2 --title "LOCKER v0.4.2 — Fase 4.2 & Endurecimiento Ponytail" --notes-file CHANGELOG.md
     ```
   - **Opción B (Interfaz web de GitHub):**
     Ir a **Releases → Draft a new release**, elegir el tag `v0.4.2`, pegar la sección `[v0.4.2]` de este `CHANGELOG.md` y pulsar **Publish release**.
