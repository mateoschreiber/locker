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
`http://127.0.0.1:18001/health/ready` consulta PostgreSQL (`503` si no responde).
Este perfil usa una instancia PostgreSQL **separada y temporal** en memoria:
sus datos se pierden al detenerla. No accede a la base del laboratorio, no usa
MQTT, no hace seed ni ejecuta migraciones. Python sigue atendiendo todas las
operaciones; Rust todavía no implementa préstamos ni el panel.

Variables Rust: `RUST_DATABASE_URL` es la conexión PostgreSQL obligatoria;
`RUST_BIND_ADDRESS` es el socket y por defecto `127.0.0.1:8001` al ejecutar
el binario directamente; Compose usa `0.0.0.0:8001` dentro del contenedor y
publica únicamente `127.0.0.1:18001` en el host;
`RUST_DB_MAX_CONNECTIONS` limita el pool (predeterminado `5`);
`RUST_DB_TIMEOUT_SECONDS` limita adquisición y consulta de salud (predeterminado
`3`, rango permitido de `1` a `30` segundos). El healthcheck Docker dispone de
`40` segundos, al menos diez de margen; `RUST_LOG` controla el filtro de logs
estructurados (Compose usa `info`).
Compose fija `RUST_DATABASE_URL` a la base temporal aislada y permite configurar
los límites y `RUST_LOG` en `.env`. No incluir credenciales reales en el repositorio.
`LOCKER_TEST_DATABASE_URL` apunta a una base aislada para la prueba de integración
Rust y solo se necesita al ejecutar esa prueba.

Las pruebas de integración crean PostgreSQL y MQTT aislados, sin publicar puertos
ni usar los volúmenes del laboratorio:

```bash
docker compose -p locker-phase0-test -f compose.test.yaml up --build --abort-on-container-exit --exit-code-from tests
docker compose -p locker-phase0-test -f compose.test.yaml down -v
python scripts/test_docker_restart.py
```

Para revisar una base existente sin cambiarla, utilice una cuenta PostgreSQL de
solo lectura y ejecute `psql -v ON_ERROR_STOP=1 -f apps/api/integrity.sql` contra
la base indicada expresamente. El informe muestra conteos, no corrige datos;
operaciones pendientes o fallidas requieren revisión y no necesariamente son corrupción.

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

## Panel Fase 2

El panel administrativo está disponible en español en `http://127.0.0.1:8083`.
Incluye inicio, operación simulada, inventario, lockers, administración y
actividad. El acceso de laboratorio es `admin` / `admin`.

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
`cargo test --locked -- --ignored`. CI proporciona esa base; sin ella la prueba
aparece como ignorada, no como validación de conexión.

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) antes de enviar cambios y
[SECURITY.md](SECURITY.md) para reportes de seguridad.
