# Locker

Laboratorio open source para un sistema de control, custodia y trazabilidad de
herramientas mediante lockers electrónicos. Esta primera versión no controla
hardware físico: usa PostgreSQL, FastAPI, React, MQTT y un simulador. La Fase 1
agrega un backend Rust experimental e independiente con Axum, Tokio y SQLx.

## Requisitos

- Docker Desktop o Docker Engine con Compose y acceso a las imágenes necesarias.
- Puerto TCP 8083 libre para el panel local. El perfil Rust opcional usa 18001.
- Node.js 22+ para las comprobaciones web locales. Rust y las herramientas de
  Python se ejecutan dentro de contenedores Linux, por lo que no hace falta
  ningún toolchain local: `scripts/check-local.ps1` se encarga de todo.

## Inicio rápido

```bash
git clone git@github.com:mateoschreiber/locker.git
cd locker
cp .env.example .env
docker compose up -d --build
docker compose exec api python -m app.modules.seed
```

Abra `http://127.0.0.1:8083`. La documentación OpenAPI está disponible
en `/docs` y los checks de salud en `/health/live` y `/health/ready`.

El panel temporal de administración usa `admin` como usuario y `admin` como
contraseña. El seed carga el laboratorio `LAB` con 24 compartimientos y 24
herramientas ficticias. La inicialización es explícita, requiere `APP_ENV=lab`
y una base de dominio vacía; no debe repetirse sobre una base con datos.

Para detener el laboratorio sin borrar datos:

```bash
docker compose down
```

Para iniciar nuevamente, reconstruir o consultar registros:

```bash
docker compose up -d
docker compose up -d --build
docker compose ps
docker compose logs -f api
```

Los servicios de PostgreSQL y MQTT están aislados dentro de Docker; no se
publican en el host. Los valores de `.env.example` son exclusivamente de
laboratorio y no deben utilizarse en producción.

El panel escucha solo en el propio equipo. Para una LAN privada se puede fijar
`LOCKER_BIND_ADDRESS` a la dirección privada concreta antes de iniciar Compose;
controle el acceso con el firewall. Las credenciales de laboratorio no son seguras.
En Windows se puede obtener la IPv4 de la interfaz activa con `ipconfig`. La
URL LAN sería `http://<IPv4-privada>:8083` tras configurar el bind y firewall;
no se ha validado desde otro dispositivo. No usar `0.0.0.0`, exponer a Internet
ni conectar equipos físicos mientras los riesgos de seguridad sigan abiertos.

## Backend Rust opcional

```bash
docker compose --profile rust up -d --build api-rust
docker compose --profile rust logs -f api-rust
docker compose --profile rust stop api-rust rust-postgres
```

`http://127.0.0.1:18001/health/live` comprueba el proceso y
`http://127.0.0.1:18001/health/ready` consulta PostgreSQL y la suscripción MQTT
(`503` si cualquiera falla). Este perfil usa una instancia PostgreSQL
**separada y temporal** en memoria: sus datos se pierden al detenerla y no
incluye esquema ni seed (el puente reintenta la recuperación hasta que el
esquema exista). Desde la Fase 2, Rust implementa el dominio funcional
completo bajo `/api/v1` (autenticación, administración, inventario,
autorizaciones, préstamos, devoluciones, operaciones, auditoría y dashboard)
con transacciones, concurrencia e idempotencia. Desde la Fase 3, Rust también
publica y confirma aperturas por MQTT contra el mismo broker que Python, con
el mismo protocolo y correlación; la propiedad de cada operación está
particionada por la columna `origin` y por los estados reclamados, por lo que
ningún backend procesa la operación del otro (ver
[protocolo](docs/hardware/HARDWARE_PROTOCOL.md)). **Python sigue siendo el
único backend conectado al laboratorio**: no apunte ambos a la misma base
mutable salvo en las pruebas de coexistencia.

Variables Rust: `RUST_DATABASE_URL` es la conexión PostgreSQL obligatoria;
`RUST_BIND_ADDRESS` es el socket y por defecto `127.0.0.1:8001` al ejecutar
el binario directamente; Compose usa `0.0.0.0:8001` dentro del contenedor y
publica únicamente `127.0.0.1:18001` en el host;
`RUST_DB_MAX_CONNECTIONS` limita el pool (predeterminado `5`);
`RUST_DB_TIMEOUT_SECONDS` limita adquisición y consulta de salud (predeterminado
`3`, rango permitido de `1` a `30` segundos). El healthcheck Docker dispone de
`40` segundos, al menos diez de margen; `RUST_LOG` controla el filtro de logs
estructurados (Compose usa `info`). Variables MQTT del puente Rust:
`RUST_MQTT_ENABLED` (`true` por defecto; `false` deja las operaciones
`PENDING` como en Fase 2, usado por la paridad REST), `RUST_MQTT_HOST` /
`RUST_MQTT_PORT` (el broker compartido), `RUST_MQTT_CLIENT_ID`
(predeterminado `locker-api-rust`) y `RUST_MQTT_CONFIRM_TIMEOUT_SECONDS`
(predeterminado `60`, rango `1` a `600`) que vence `PUBLISHED` a `UNKNOWN`
sin liberar recursos.
Compose fija `RUST_DATABASE_URL` a la base temporal aislada y permite configurar
los límites y `RUST_LOG` en `.env`. No incluir credenciales reales en el repositorio.
`LOCKER_TEST_DATABASE_URL` apunta a una base aislada para las pruebas de
integración Rust (salud, dominio y MQTT) y solo se necesita al ejecutarlas;
`LOCKER_TEST_MQTT_HOST` apunta al broker aislado de esas pruebas.

Las pruebas de integración crean PostgreSQL y MQTT aislados, sin publicar puertos
ni usar los volúmenes del laboratorio:

```bash
docker compose -p locker-phase0-test -f compose.test.yaml up --build --abort-on-container-exit --exit-code-from tests
docker compose -p locker-phase0-test -f compose.test.yaml down -v
python scripts/test_docker_restart.py
```

La paridad de contratos Python/Rust se comprueba con ambos backends contra la
misma base aislada (listados, flujo de escritura, errores y detalle de mensajes),
con el puente MQTT de Rust apagado para comparar solo los contratos REST:

```bash
python scripts/test_phase2_parity.py
```

El ciclo MQTT completo de Rust se valida con PostgreSQL, Mosquitto, el
simulador real y procesos reales: flujo completo, evento duplicado, caída y
recuperación del broker, reinicio del backend a mitad de una operación
(republicación segura de `PENDING`, `UNKNOWN` para lo incierto, conciliación
por REST) y coexistencia Python/Rust sobre una base compartida con propiedad
única por operación:

```bash
python scripts/test_rust_mqtt.py
```

Para revisar una base existente sin cambiarla, utilice una cuenta PostgreSQL de
solo lectura y ejecute `psql -v ON_ERROR_STOP=1 -f apps/api/integrity.sql` contra
la base indicada expresamente. El informe muestra conteos, no corrige datos;
operaciones `UNKNOWN` o fallidas requieren conciliación (ver
[protocolo](docs/hardware/HARDWARE_PROTOCOL.md)) y no necesariamente son corrupción.

## Arquitectura

```text
Browser → web (Nginx :8083) → API (FastAPI) → PostgreSQL
                              ↘ MQTT (Mosquitto) ↔ Locker Simulator
```

El backend será un monolito modular. El simulador implementa el límite de
hardware mediante MQTT, por lo que una futura integración física no deberá
incorporar lógica específica de fabricante al dominio.

La documentación de arquitectura, decisiones y protocolo se encuentra en
[`docs/`](docs/).

## Panel Fase 2 (interfaz)

El panel administrativo está disponible en español en `http://127.0.0.1:8083`.
Incluye inicio, operación simulada, inventario, lockers, administración y
actividad. El acceso de laboratorio es `admin` / `admin`.

## Contratos de administración (Fase 4.0)

- Los estados administrativos se validan contra una lista explícita por
  entidad (`ACTIVE/INACTIVE` en sucursales, usuarios, membresías y
  compartimentos; `ONLINE/INACTIVE` en lockers; `LOCKED/INACTIVE` en
  cerraduras; `SIMULATED/INACTIVE` en cámaras; `AVAILABLE/INACTIVE` en
  herramientas). Un estado desconocido responde `422` con el mismo mensaje en
  ambos backends; el panel ofrece la lista, no texto libre.
- Los estados de dominio no son editables manualmente: `ON_LOAN` lo fija
  únicamente la confirmación de retiro, y préstamos, autorizaciones y
  operaciones no tienen rutas de escritura administrativa.
- Una herramienta con préstamo activo o pendiente, y un compartimento con
  ubicación activa o devolución pendiente, rechazan el cambio manual de
  estado y la desactivación con `409` (con bloqueo transaccional, igual que
  checkout y devolución).
- Ninguna respuesta de usuarios o autenticación incluye el atributo
  `password`, en ninguno de los backends.
- `POST /api/v1/operations/{id}/resolve` acepta, además de una operación
  `UNKNOWN`, una operación `FAILED` que aún retiene un préstamo pendiente
  (fallo de confirmación con apertura físicamente posible). La reversión del
  préstamo exige la atestación del operador con motivo y evidencia; nunca es
  automática. Un fallo de publicación sin entrega (p. ej. código de locker
  ambiguo) revierte préstamo y autorización en la misma transacción.
- La reserva de un compartimento durante una devolución cubre todos los
  estados en curso (`PENDING`, `PUBLISHING`, `PUBLISHED`, `UNKNOWN`); una
  colocación nueva en ese compartimento responde `409`.

## Supervisión y presencia (Fase 4.1)

El backend Rust expone dos contratos adicionales que Python no implementa;
`GET /api/v1/capabilities` los declara y el panel solo habilita esas
funciones cuando el backend conectado los anuncia:

- `GET /api/v1/lockers/presence`: presencia observada por locker mediante
  MQTT (`LOCKER_ONLINE`/`LOCKER_HEARTBEAT`/`LOCKER_OFFLINE` en
  `locker/{code}/status`). Estados: `ONLINE` (comunicación reciente y
  válida), `STALE` (evidencia vencida — un mensaje retained antiguo nunca se
  presenta como conectado), `OFFLINE` (desconexión conocida, incluido el
  Last Will) y `UNKNOWN` (sin información confiable o código duplicado entre
  sucursales, `ambiguous_code`). Incluye `last_communication`,
  `age_seconds`, `source` y `updated_at` cuando existen. La presencia vive en
  memoria del proceso y se recupera tras reinicios por el replay retained del
  broker, revalidando timestamps; los heartbeats no generan auditoría.
- `RUST_PRESENCE_STALE_SECONDS` (predeterminado `90`, rango `30`–`600`)
  fija la ventana a partir de la cual la comunicación se considera `STALE`;
  configurable en `.env` para el perfil `rust`.

## Panel de operaciones y conciliación (Fase 4.1)

La vista **Operaciones** del panel supervisa el ciclo de vida completo:
estado, tipo, herramienta, locker/compartimento, usuario, correlación,
origen y fechas de solicitud, publicación y confirmación, con filtro por
estado y detalle expandible que conserva los identificadores técnicos. Las
operaciones que requieren intervención (UNKNOWN, o FAILED que aún retiene un
préstamo pendiente) se marcan y ordenan primero; el dashboard las refleja.

La conciliación usa el endpoint Rust `POST /operations/{id}/resolve` con el
token de la sesión de laboratorio (nunca una constante en el código): exige
veredicto, motivo y evidencia, explica las consecuencias antes de ejecutar y
muestra el estado resultante real de la respuesta del backend — un veredicto
CONFIRMED cuyas precondiciones fallan se informa como FAILED con su motivo.
Solo se ofrece para operaciones elegibles; los rechazos del backend (401,
409, 422, 503) y los fallos de red se presentan con su mensaje. Python no
implementa `resolve`: el panel lo detecta por `GET /api/v1/capabilities` y
nunca envía esa solicitud al backend conectado si no la anuncia.

## Entorno aislado Rust-preview (Fase 4.1)

El panel React puede probarse completo contra el backend Rust sin tocar el
laboratorio activo: proyecto, red, broker y volúmenes propios (nada compartido
con el stack principal; el broker aislado impide que los comandos lleguen al
simulador del laboratorio). La base se inicializa explícitamente con el mismo
esquema y seed del laboratorio, y el nginx del preview apunta a Rust mediante
`apps/web/nginx/rust.conf` sin modificar la configuración productiva. Ese
nginx resuelve el nombre `api-rust` en cada petición (DNS embebido de Docker,
`resolver 127.0.0.11`): un reinicio del backend le asigna otra IP de red y
una resolución estática cacheada dejaría el proxy marcando la IP muerta para
siempre (fallo real del CI #23, ver Plan Maestro).

El archivo Compose publica el puerto con la variable `LOCKER_PREVIEW_PORT`
(por defecto 8084), por lo que el mismo archivo sirve a los dos entornos sin
duplicación ni riesgo mutuo:

| Entorno | Proyecto Compose | Puerto | Datos |
|---|---|---|---|
| Preview interactivo | `locker-rust-preview` | 8084 | persistentes entre start/stop |
| Smoke automatizado | `locker-rust-preview-test` | 8085 (`LOCKER_PREVIEW_TEST_PORT`) | desechables (`down -v` solo de este proyecto) |
| Laboratorio Python | `locker` | 8083 | nunca tocado por ninguno de los dos |

### Uso interactivo (Windows)

`scripts/rust-preview.ps1` gestiona el preview desde cualquier directorio de
PowerShell; fija el proyecto `locker-rust-preview` y el puerto 8084, y el
laboratorio Python (8083) permanece independiente e intacto:

```powershell
.\scripts\rust-preview.ps1 start    # levanta y espera readiness; inicializa
                                   # esquema + seed LAB solo la primera vez
                                   # (nunca re-sembrará una base con datos)
.\scripts\rust-preview.ps1 stop     # detiene conservando los datos
.\scripts\rust-preview.ps1 status   # contenedores y salud del preview
.\scripts\rust-preview.ps1 init     # inicialización explícita (el seed
                                   # rechaza bases no vacías)
.\scripts\rust-preview.ps1 reset    # destrucción explícita de datos y
                                   # volúmenes (solo cuando se desea)
```

Con el preview iniciado, abra `http://127.0.0.1:8084` (perfil de solo
laboratorio, `admin`/`admin`): presencia observada de los lockers, vista de
Operaciones y conciliación administrativa contra Rust. Una parada normal
conserva los datos; `reset` es la única acción que los elimina.

### Escenario automatizado

El ciclo completo —capacidades, presencia, flujo de préstamo, UNKNOWN
por TTL, pérdida y reinicio real del broker, conciliación con atestación
(verificando el estado efectivo del préstamo y de la autorización contra la
base, no solo respuestas HTTP) y reinicio real del backend a mitad de
operación— se valida con:

```bash
python scripts/test_rust_preview.py
```

El smoke corre bajo su propio proyecto `locker-rust-preview-test` en el
puerto 8085 (`LOCKER_PREVIEW_TEST_PORT`) y solo destruye ese proyecto; si el
preview interactivo está corriendo, toma una instantánea de solo lectura de
sus datos y afirma al final que quedaron intactos. Ante cualquier fallo
captura evidencia diagnóstica (contenedores, registros de api-rust,
postgres, mosquitto y nginx, y el modo de fallo de `health/live` y
`health/ready`: 502/503/timeout/rechazo) en `.local-validation/` **antes**
del desmonte desechable, para poder diagnosticar a posteriori.
`LOCKER_PREVIEW_KEEP=1` conserva el entorno levantado para depuración. TTL
de confirmación corto (`RUST_MQTT_CONFIRM_TIMEOUT_SECONDS: 5`) para que los
escenarios inciertos sean conciliables rápidamente.

## Validación local (Fase 4.1-D)

La validación principal del repositorio es local y reproducible; GitHub
Actions queda como comprobación adicional bajo demanda. Desde cualquier
directorio de PowerShell:

```powershell
.\scripts\check-local.ps1            # QUICK (default): fmt, clippy, pruebas
                                      # unitarias Rust (contenedor Linux),
                                      # lint/build/test web, diff staged+unstaged
.\scripts\check-local.ps1 FULL       # QUICK + integración Rust con PostgreSQL
                                      # y Mosquitto desechables, escenarios MQTT,
                                      # config Compose, smoke de imagen, checks
                                      # Python (ruff/mypy/pytest), paridad y
                                      # regresión de reinicio
.\scripts\check-local.ps1 E2E        # FULL + preview guardián desechable con
                                      # marcador de persistencia, smoke completo
                                      # (login, préstamos, presencia, reinicios,
                                      # UNKNOWN/FAILED, conciliación) y
                                      # verificación de que el laboratorio 8083
                                      # y el preview interactivo quedan intactos
```

Cada paso reporta PASS, FAIL o SKIPPED con su motivo, se registra la duración,
la evidencia de fallos queda en `.local-validation/`, el código de salida es
no cero si algo falla, y las dependencias Rust/Python se cachean en volúmenes
Docker para no reinstalarlas en cada corrida. Ningún modo toca los datos del
laboratorio (8083), del preview interactivo (8084) ni de sus volúmenes.

## Desarrollo

El flujo de trabajo es: desarrollar con OpenCode → ejecutar la validación
local (`scripts/check-local.ps1`) → revisión → commit en `main` → push
opcional → CI remoto manual desde la pestaña Actions cuando se desee una
comprobación completa (el workflow es `workflow_dispatch`; ningún push
ejecuta los jobs automáticamente).

Las comprobaciones equivalentes por aplicación, dentro o fuera de contenedores:

```bash
cd apps/api && pip install -e '.[dev]' && ruff check . && mypy app && pytest
cd apps/web && npm ci && npm run lint && npm run build && npm test
cd apps/locker-simulator && pip install -e '.[dev]' && ruff check . && pytest
cd apps/api-rust && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && cargo build --release --locked
```

La prueba Rust con PostgreSQL se ejecuta explícitamente con
`LOCKER_TEST_DATABASE_URL` apuntando a una base aislada y
`cargo test --locked -- --ignored`; cubre salud y las 8 pruebas del dominio
(auth/admin, colocaciones, autorizaciones, checkout idempotente, concurrencia
de retiros y de devoluciones, devoluciones y dashboard). `check-local.ps1
FULL` levanta esa base y su broker automáticamente; sin ella la prueba
aparece como ignorada, no como validación de conexión.

**Reglas de rama:** si `main` tuviera checks obligatorios configurados en
GitHub (Settings → Branches), deben retirarse o actualizarse al pasar CI a
ejecución manual; de lo contrario un push quedaría esperando checks que ya
no se disparan automáticamente. Sin permisos de administración no se puede
consultar esa configuración desde aquí; verifíquela una vez en GitHub.

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) antes de enviar cambios y
[SECURITY.md](SECURITY.md) para reportes de seguridad.
