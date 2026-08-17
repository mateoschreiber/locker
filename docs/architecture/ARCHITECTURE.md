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
