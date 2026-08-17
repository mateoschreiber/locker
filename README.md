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
```

Abra `http://<IP-del-servidor>:8083`. La documentación OpenAPI está disponible
en `/docs` y los checks de salud en `/health/live` y `/health/ready`.

El panel temporal de administración usa `admin` como usuario y `admin` como
contraseña. El seed carga el laboratorio `LAB` con 24 compartimientos y 24
herramientas ficticias.

Para detener el laboratorio sin borrar datos:

```bash
docker compose down
```

Los servicios de PostgreSQL y MQTT están aislados dentro de Docker; no se
publican en el host. Los valores de `.env.example` son exclusivamente de
laboratorio y no deben utilizarse en producción.

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

El panel administrativo está disponible en español en `http://<IP>:8083`.
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
