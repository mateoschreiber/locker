# LOCKER — Electronic Tool Custody & Traceability Lab

![Estado](https://img.shields.io/badge/Fase-4.2_Completada-2ea44f?style=flat-square)
![Rust](https://img.shields.io/badge/Rust-Axum_%7C_Tokio_%7C_SQLx-dea584?style=flat-square&logo=rust)
![Python](https://img.shields.io/badge/Python_3.12-FastAPI_%7C_SQLAlchemy-3776ab?style=flat-square&logo=python&logoColor=white)
![React](https://img.shields.io/badge/React_19-TypeScript_%7C_Vite-61dafb?style=flat-square&logo=react&logoColor=black)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-17_Alpine-336791?style=flat-square&logo=postgresql&logoColor=white)
![MQTT](https://img.shields.io/badge/MQTT-Eclipse_Mosquitto-3c5280?style=flat-square&logo=eclipsemosquitto&logoColor=white)
![Licencia](https://img.shields.io/badge/Licencia-MIT-blue?style=flat-square)

**LOCKER** es un laboratorio open-source para el control, préstamo, devolución, custodia y trazabilidad de herramientas mediante lockers electrónicos gobernados por **MQTT**.

Esta edición opera un entorno de simulación completo sin requerir hardware físico conectado e implementa una migración arquitectónica progresiva y verificada desde **FastAPI (Python)** hacia un monolito modular de alto rendimiento en **Rust (Axum + Tokio + SQLx)** con garantías transaccionales estrictas, idempotencia, detección de presencia en tiempo real y conciliación administrativa auditable.

---

## Tabla de Contenidos

1. [Arquitectura y Máquina de Estados](#1-arquitectura-y-máquina-de-estados)
2. [Comparativa de Capacidades y Entornos](#2-comparativa-de-capacidades-y-entornos)
3. [Requisitos e Inicio Rápido](#3-requisitos-e-inicio-rápido)
4. [Entorno Rust-Preview (Fase 4.1 / 4.2)](#4-entorno-rust-preview-fase-41--42)
5. [Contratos de Dominio, Presencia y Paginación](#5-contratos-de-dominio-presencia-y-paginación)
6. [Rendimiento Medido (Benchmark Fase 4.2)](#6-rendimiento-medido-benchmark-fase-42)
7. [Validación Local Reproducible (`check-local.ps1`)](#7-validación-local-reproducible-check-localps1)
8. [Configuración y Variables de Entorno](#8-configuración-y-variables-de-entorno)
9. [Releases, Documentación y Seguridad](#9-releases-documentación-y-seguridad)

---

## 1. Arquitectura y Máquina de Estados

El sistema desacopla el dominio transaccional de la frontera física del hardware mediante un protocolo de mensajería MQTT con correlación explícita y propiedad particionada por operación (`origin`).

```mermaid
flowchart LR
    Browser["Navegador Web<br/>(Panel React 19)"]
    Nginx["Nginx Reverse Proxy<br/>(DNS dinámico 127.0.0.11)"]
    ApiPy["API Python (FastAPI)<br/>Laboratorio :8083"]
    ApiRs["API Rust (Axum/SQLx)<br/>Preview :8084 / Perfil :18001"]
    PG[("PostgreSQL 17<br/>(ACID + Row Locks)")]
    MQTT[["Mosquitto Broker<br/>(MQTT QoS 1)"]]
    Sim["Simulador de Locker<br/>(Dedup 10k correlaciones)"]

    Browser -->|HTTP /api/v1| Nginx
    Nginx -->|Stack Principal| ApiPy
    Nginx -.->|Stack Preview| ApiRs
    ApiPy --> PG
    ApiRs --> PG
    ApiPy <-->|commands / events| MQTT
    ApiRs <-->|commands / events / status| MQTT
    MQTT <-->|locker/+/commands<br/>locker/+/events<br/>locker/+/status| Sim
```

### Ciclo de vida de una operación física (Fase 3 / Fase 4)

En el backend Rust, un `PUBACK` del broker MQTT **jamás** se interpreta como apertura física completada: solo acredita que el broker aceptó el comando. Cuando el resultado físico es incierto (caída de red, reinicio a mitad de vuelo o expiración del TTL), la operación pasa a `UNKNOWN` manteniendo bloqueados los recursos del dominio hasta que un operador la concilie con motivo y evidencia.

```mermaid
stateDiagram-v2
    [*] --> PENDING: Solicitud REST (checkout / return)
    PENDING --> PUBLISHING: Reclamo atómico (FOR UPDATE SKIP LOCKED)
    PUBLISHING --> FAILED: Fallo conocido antes de publicar (ej. código ambiguo)
    PUBLISHING --> PUBLISHED: PUBACK recibido del broker
    PUBLISHING --> UNKNOWN: Reinicio / desconexión sin PUBACK
    PUBLISHED --> CONFIRMED: Evento COMPARTMENT_OPENED válido
    PUBLISHED --> FAILED: Evento con precondición rota (retiene préstamo)
    PUBLISHED --> UNKNOWN: TTL expirado sin confirmación
    UNKNOWN --> CONFIRMED: Evento tardío o Conciliación CONFIRMED
    UNKNOWN --> FAILED: Conciliación FAILED (revierte préstamo)
```

---

## 2. Comparativa de Capacidades y Entornos

El panel React consulta `GET /api/v1/capabilities` tras iniciar sesión para activar automáticamente las funciones avanzadas cuando se conecta al backend Rust, manteniendo compatibilidad total con el backend Python del laboratorio.

| Capacidad / Contrato | Backend Python (`:8083`) | Backend Rust (`:8084` / `:18001`) |
| :--- | :---: | :---: |
| Dominio `/api/v1` (auth, admin, inventario, préstamos, auditoría) | Sí | Sí (100% paridad REST) |
| Validación de estados administrativos (`422`) y guardias (`409`) | Sí | Sí |
| Bloqueo de autorizaciones/retiros sobre entidades `INACTIVE` (`409`) | Sí | Sí |
| Exclusión de `password` en respuestas y optimización `locker_detail` | Sí | Sí |
| Concurrencia `FOR UPDATE` e idempotencia (`202` en reintentos) | Parcial | **Completa** |
| Outbox MQTT transaccional (`PUBLISHING`/`PUBLISHED`/`UNKNOWN`) | Fire-and-forget | **Completo con recuperación** |
| Conciliación administrativa (`POST /operations/{id}/resolve`) | No | **Sí (`resolve: true`)** |
| Presencia MQTT en tiempo real (`GET /lockers/presence`) | No | **Sí (`presence: true`)** |
| Paginación determinística (`?limit=&offset=`) | No | **Sí (`pagination: true`)** |
| Operaciones enriquecidas en un solo `JOIN` + filtro `attention=1\|0` | No | **Sí (`enriched_operations: true`)** |

### Matriz de entornos Docker aislados

| Entorno | Proyecto Compose | Archivo Compose | Puerto Host | Persistencia de Datos |
| :--- | :--- | :--- | :---: | :--- |
| **Laboratorio Python** | `locker` | `compose.yaml` | `127.0.0.1:8083` | Persistente (nunca tocado por pruebas) |
| **Perfil Rust opcional** | `locker` (`--profile rust`) | `compose.yaml` | `127.0.0.1:18001` | `tmpfs` en memoria (solo API directa) |
| **Preview Interactivo Rust** | `locker-rust-preview` | `compose.rust-preview.yaml` | `127.0.0.1:8084` | Persistente entre `start` y `stop` |
| **Smoke Automatizado Rust** | `locker-rust-preview-test` | `compose.rust-preview.yaml` | `127.0.0.1:8085` | Desechable (`down -v` automático) |
| **Benchmark / Guardián E2E** | `locker-rust-bench` / `guard` | `compose.rust-preview.yaml` | `127.0.0.1:8086` | Desechable |

---

## 3. Requisitos e Inicio Rápido

### Requisitos
- **Docker Desktop** o **Docker Engine** con Docker Compose.
- **Node.js 22+** para comprobaciones locales del frontend. *(Los toolchains de Rust y Python se ejecutan dentro de contenedores Linux mediante `scripts/check-local.ps1`, sin requerir instalación local).*
- Puertos TCP locales libres: `8083` (laboratorio), `8084` (preview Rust), `18001` (API Rust opcional).

### Levantar el laboratorio principal (`127.0.0.1:8083`)

```bash
git clone git@github.com:mateoschreiber/locker.git
cd locker
cp .env.example .env
docker compose up -d --build
docker compose exec api python -m app.modules.seed
```

- **Panel web:** `http://127.0.0.1:8083` (credenciales de laboratorio: `admin` / `admin`).
- **Documentación OpenAPI:** `http://127.0.0.1:8083/docs`
- **Checks de salud:** `/health/live` y `/health/ready`
- **Semilla inicial (`seed`):** carga la sucursal `LAB` con 1 locker (`LAB-LOCKER-001`), 24 compartimentos y 24 herramientas ficticias. Es explícita, exige `APP_ENV=lab` y una base vacía (jamás sobrescribe bases con datos).

```bash
# Operaciones habituales del laboratorio
docker compose ps
docker compose logs -f api
docker compose down          # Detiene contenedores conservando los volúmenes de datos
```

---

## 4. Entorno Rust-Preview (Fase 4.1 / 4.2)

El panel React completo puede ejecutarse contra el backend **Rust** en un stack totalmente aislado (red, broker Mosquitto, simulador y volúmenes propios en el puerto `8084`) sin interferir con el laboratorio Python (`8083`).

### Gestión interactiva en Windows (`scripts/rust-preview.ps1`)

```powershell
.\scripts\rust-preview.ps1 start    # Levanta stack, espera readiness e inicializa esquema + seed LAB la primera vez
.\scripts\rust-preview.ps1 status   # Muestra contenedores y estado de salud del preview
.\scripts\rust-preview.ps1 stop     # Detiene el preview conservando sus datos
.\scripts\rust-preview.ps1 init     # Inicialización explícita (rechaza bases no vacías)
.\scripts\rust-preview.ps1 reset    # Destruye contenedores y volúmenes del preview para empezar limpio
```

Una vez iniciado, abre `http://127.0.0.1:8084` (`admin` / `admin`) para probar la presencia MQTT en vivo, la vista **Operación** (crear, aprobar, retirar, cancelar y devolver), la vista **Operaciones** enriquecida y la conciliación administrativa.

### Smoke automatizado del preview (`scripts/test_rust_preview.py`)

```bash
python scripts/test_rust_preview.py
```

Ejecuta en el proyecto desechable `locker-rust-preview-test` (puerto `8085`) el ciclo extremo a extremo a través de Nginx: readiness, capacidades, login, presencia `ONLINE` por latido, préstamo y devolución confirmados, caída del simulador (`Last Will → OFFLINE`), reinicio real del broker con recuperación por replay `retained`, expiración de TTL a `UNKNOWN` conciliado con atestación y reinicio real de `api-rust` a mitad de operación. Ante cualquier fallo, guarda evidencia diagnóstica en `.local-validation/`.

### Perfil Rust opcional dentro del Compose principal (`127.0.0.1:18001`)

```bash
docker compose --profile rust up -d --build api-rust
docker compose --profile rust logs -f api-rust
docker compose --profile rust stop api-rust rust-postgres
```

Usa una instancia `rust-postgres` separada en memoria (`tmpfs`, sin esquema ni seed automáticos; el puente MQTT espera silenciosamente hasta que el esquema exista). **Regla de consumidor único:** no apuntes ambos backends a la misma base mutable fuera de las pruebas de coexistencia.

---

## 5. Contratos de Dominio, Presencia y Paginación

### Contratos de administración e integridad (Fase 4.0 + F4.2-fix)
- **Lista blanca de estados administrativos por entidad:**
  - `ACTIVE` / `INACTIVE`: sucursales, usuarios, membresías y compartimentos.
  - `ONLINE` / `INACTIVE`: lockers.
  - `LOCKED` / `INACTIVE`: cerraduras.
  - `SIMULATED` / `INACTIVE`: cámaras.
  - `AVAILABLE` / `INACTIVE`: herramientas.
  - Cualquier estado desconocido o exclusivo del dominio (`ON_LOAN`) responde `422 Unprocessable Entity` con mensaje idéntico en ambos backends.
- **Guardias de estado activo (`409 Conflict`):**
  - Una herramienta con préstamo activo o pendiente y un compartimento ocupado o reservado por una devolución en curso (`PENDING`, `PUBLISHING`, `PUBLISHED`, `UNKNOWN`) rechazan cambios manuales de estado y desactivación (`PATCH` y `/deactivate`) con bloqueo transaccional `FOR UPDATE`.
  - No se permite crear autorizaciones (`POST /authorizations`) ni ejecutar retiros (`POST /authorizations/{id}/checkout`) cuando la herramienta, usuario, sucursal, locker o compartimento tienen estado `INACTIVE` (`409 "No se puede operar con una entidad inactiva"`).
- **Protección de credenciales:** ninguna respuesta REST incluye jamás el campo `password`.
- **Conciliación (`POST /api/v1/operations/{id}/resolve`):** acepta operaciones `UNKNOWN` y operaciones `FAILED` que aún retienen un préstamo pendiente (`CHECKOUT_PENDING` / `RETURN_PENDING`). Exige `Authorization: Bearer lab-admin-token`, `outcome` (`CONFIRMED` | `FAILED`), `reason` (≤255 caracteres) y `evidence`.

### Supervisión y presencia MQTT (Fase 4.1)
- `GET /api/v1/lockers/presence`: consume `locker/{code}/status` en memoria y deriva en tiempo de lectura:
  - `ONLINE`: comunicación reciente dentro de `RUST_PRESENCE_STALE_SECONDS` (90 s por defecto).
  - `STALE`: evidencia vencida (un mensaje `retained` antiguo jamás se presenta como conectado).
  - `OFFLINE`: desconexión conocida o Last Will del dispositivo.
  - `UNKNOWN`: sin evidencia o código de locker duplicado entre sucursales (`ambiguous_code: true`).

### Paginación opt-in, operaciones enriquecidas y carga por vista (Fase 4.2)
- **Paginación (`/{entity}?limit=&offset=`):** con `limit` (`1..=200`) y `offset` (`≥0`), el backend Rust devuelve `{ items, total, limit, offset }` ordenado por `created_at DESC, id DESC`. `offset` sin `limit` o valores fuera de rango responden `422`. Sin `limit`, devuelve el array completo legado.
- **Operaciones enriquecidas (`GET /api/v1/operations?limit=`):** cada fila incluye en una sola consulta `JOIN` `tool_name`, `locker_code`, `compartment_code`, `user_display_name`, `loan_status` y `requires_intervention`. Soporta el filtro `attention=1|0` (requiere `limit`).
- **Sondeo del panel por vista activa (`apps/web/src/polling.ts`):** el frontend solo sondea los recursos de la vista en pantalla (≤3 peticiones cada 4 s), cachea las tablas de referencia (`branches`, `roles`, `users`, `lockers`, `compartments`) refrescándolas tras cada mutación, pausa el sondeo cuando la pestaña está oculta y aborta peticiones en vuelo al cambiar de vista.

---

## 6. Rendimiento Medido (Benchmark Fase 4.2)

Resultados obtenidos con `scripts/bench_panel.py` sobre el stack aislado (`127.0.0.1:8086`). El detalle metodológico completo reside en [`docs/PERFORMANCE_BASELINE.md`](docs/PERFORMANCE_BASELINE.md).

| Métrica | Pre-F4.2 (Sin paginar, ciclo completo) | Post-F4.2 (Paginado `limit=100`, por vista) | Mejora |
| :--- | :---: | :---: | :---: |
| **Solicitudes al backend por minuto** | ~245 req/min (16 req / 4 s) | **45–48 req/min** (≤3 req / 4 s) | **−80 %** |
| **Tráfico por ciclo (Dataset 1x — seed LAB)** | ~31,0 KB | **≤6,1 KB** | **−80 %** |
| **Latencia p95 `/api/v1/audit` (Dataset 100x — 24 000 filas)** | 264,6 ms (6 034,9 KB/resp) | **28,7 ms (25,3 KB/resp)** | **−89 % latencia / −99,6 % bytes** |
| **RAM y CPU de `api-rust` (Dataset 100x)** | 80–129 MiB (picos ~22 % CPU) | **5,4–6,2 MiB planos** (≤0,74 % CPU) | **−95 % RAM** |

---

## 7. Validación Local Reproducible (`check-local.ps1`)

La validación principal del proyecto es local y determinística desde cualquier directorio de PowerShell (GitHub Actions permanece en modo manual `workflow_dispatch`):

```powershell
.\scripts\check-local.ps1            # QUICK (por defecto): cargo fmt, clippy -D warnings,
                                     # unitarias Rust, lint/build/vitest web y git diff --check
.\scripts\check-local.ps1 FULL       # QUICK + integración Rust (PostgreSQL + Mosquitto aislados),
                                     # escenarios MQTT, Compose checks, smoke Docker, ruff/mypy/pytest
                                     # de API Python y simulador, paridad REST y reinicio Docker
.\scripts\check-local.ps1 E2E        # FULL + preview guardián con marcador de persistencia, smoke E2E
                                     # completo y verificación de integridad del lab 8083 y preview 8084
```

### Scripts individuales de verificación e integridad

```bash
# Paridad de contratos REST Python vs Rust (59 comparaciones sobre base aislada)
python scripts/test_phase2_parity.py

# Ciclo MQTT completo en Rust (flujo, duplicados, caída de broker, reinicio mid-operation y coexistencia)
python scripts/test_rust_mqtt.py

# Regresión de persistencia tras reinicio real de contenedor Docker
python scripts/test_docker_restart.py

# Auditoría SQL de solo lectura sobre una base existente
psql -v ON_ERROR_STOP=1 -f apps/api/integrity.sql
```

---

## 8. Configuración y Variables de Entorno

Copia `.env.example` a `.env` para el laboratorio local. Nunca incluyas credenciales de producción en el repositorio.

| Variable | Servicio | Valor por defecto (Lab) | Descripción |
| :--- | :--- | :--- | :--- |
| `APP_ENV` | `api` | `lab` | Entorno de ejecución (`lab` habilita la ejecución explícita del seed). |
| `LOCKER_BIND_ADDRESS` | `web` | `127.0.0.1` | IP del host donde se publica el puerto `8083` (no usar `0.0.0.0`). |
| `LOCKER_PREVIEW_PORT` | `web-rust` | `8084` | Puerto host para el stack `compose.rust-preview.yaml` (`8085` en smoke test). |
| `DATABASE_URL` | `api` | `postgresql+psycopg://...` | Cadena de conexión SQLAlchemy/Alembic hacia PostgreSQL. |
| `MQTT_HOST` / `MQTT_PORT` | `api`, `simulator` | `mosquitto` / `1883` | Host y puerto del broker MQTT interno. |
| `LOCKER_ID` | `locker-simulator` | `LAB-LOCKER-001` | Código de locker que simula el contenedor `locker-simulator`. |
| `RUST_DATABASE_URL` | `api-rust` | *(obligatoria)* | Cadena de conexión PostgreSQL para el pool SQLx de Rust. |
| `RUST_BIND_ADDRESS` | `api-rust` | `127.0.0.1:8001` | Socket de escucha del binario Rust (`0.0.0.0:8001` dentro de Docker). |
| `RUST_DB_MAX_CONNECTIONS` | `api-rust` | `5` | Tamaño máximo del pool de conexiones PostgreSQL en Rust (> 0). |
| `RUST_DB_TIMEOUT_SECONDS` | `api-rust` | `3` | Timeout de adquisición y healthcheck SQL en segundos (rango `1..=30`). |
| `RUST_MQTT_ENABLED` | `api-rust` | `true` | Activa el puente MQTT (`false` deja operaciones `PENDING` para tests REST). |
| `RUST_MQTT_HOST` / `PORT` | `api-rust` | `127.0.0.1` / `1883` | Dirección del broker MQTT para el puente Rust. |
| `RUST_MQTT_CLIENT_ID` | `api-rust` | `locker-api-rust` | Prefijo del `client_id` MQTT (se le añade un sufijo UUID por proceso). |
| `RUST_MQTT_CONFIRM_TIMEOUT_SECONDS` | `api-rust` | `60` | TTL tras `PUBACK` antes de marcar la operación como `UNKNOWN` (`1..=600`). |
| `RUST_PRESENCE_STALE_SECONDS` | `api-rust` | `90` | Ventana en segundos antes de degradar la presencia a `STALE` (`30..=600`). |
| `RUST_LOG` | `api-rust` | `info` | Filtro de nivel para logs estructurados JSON (`tracing`). |
| `LOCKER_TEST_DATABASE_URL` | Tests Rust | *(solo tests)* | Base aislada para `cargo test --locked -- --ignored`. |
| `LOCKER_TEST_MQTT_HOST` | Tests Rust | *(solo tests)* | Broker Mosquitto aislado para las pruebas de integración Rust. |

---

## 9. Releases, Documentación y Seguridad

- **Historial de versiones y notas de lanzamiento:** consulta [`CHANGELOG.md`](CHANGELOG.md) para ver el detalle de cada versión (`v0.1.0` a `v0.4.2`) y las instrucciones para etiquetar y publicar manualmente en **GitHub Releases**.
- **Plan Maestro y Auditoría:** el seguimiento detallado de fases y hallazgos se encuentra en [`PLAN_MAESTRO_MIGRACION_RUST.md`](PLAN_MAESTRO_MIGRACION_RUST.md) y [`RESUMEN_AUDITORIA.md`](RESUMEN_AUDITORIA.md).
- **Documentación técnica adicional:** revisa [`docs/`](docs/) para arquitectura, ADRs, modelo de dominio, [`docs/hardware/HARDWARE_PROTOCOL.md`](docs/hardware/HARDWARE_PROTOCOL.md) y [`docs/PERFORMANCE_BASELINE.md`](docs/PERFORMANCE_BASELINE.md).
- **Seguridad y Contribuciones:** consulta [`SECURITY.md`](SECURITY.md) y [`CONTRIBUTING.md`](CONTRIBUTING.md).

> **Aviso de Seguridad de Laboratorio:** Las credenciales `admin` / `admin`, el token de laboratorio y el broker MQTT anónimo sin TLS son exclusivos para desarrollo y simulación local aislada. **No expongas este entorno a Internet ni conectes cerraduras o activos físicos reales** hasta completar el endurecimiento de la Fase 5.
