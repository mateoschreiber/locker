# Locker

Laboratorio open source para un sistema de control, custodia y trazabilidad de
herramientas mediante lockers electrónicos. Esta primera versión no controla
hardware físico: usa PostgreSQL, FastAPI, React, MQTT y un simulador. La Fase 1
agrega un backend Rust experimental e independiente con Axum, Tokio y SQLx.

## Requisitos

- Docker Desktop o Docker Engine con Compose y acceso a las imágenes necesarias.
- Puerto TCP 8083 libre para el panel local. El perfil Rust opcional usa 18001.
- Rust estable con `rustfmt` y `clippy` solo para compilar y probar fuera de Docker.

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

## Desarrollo

Ejecute las validaciones desde cada aplicación:

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
de retiros y de devoluciones, devoluciones y dashboard). CI proporciona esa base; sin ella la prueba
aparece como ignorada, no como validación de conexión.

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) antes de enviar cambios y
[SECURITY.md](SECURITY.md) para reportes de seguridad.
