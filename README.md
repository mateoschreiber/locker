# Locker

Laboratorio open source para un sistema de control, custodia y trazabilidad de
herramientas mediante lockers electrónicos. Esta primera versión no controla
hardware físico: usa PostgreSQL, FastAPI, React, MQTT y un simulador.

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

Los servicios de PostgreSQL y MQTT están aislados dentro de Docker; no se
publican en el host. Los valores de `.env.example` son exclusivamente de
laboratorio y no deben utilizarse en producción.

El panel escucha solo en el propio equipo. Para una LAN privada se puede fijar
`LOCKER_BIND_ADDRESS` a la dirección privada concreta antes de iniciar Compose;
controle el acceso con el firewall. Las credenciales de laboratorio no son seguras.

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
```

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) antes de enviar cambios y
[SECURITY.md](SECURITY.md) para reportes de seguridad.
