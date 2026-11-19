# Despliegue de laboratorio

El stack de Fase 0 publica el puerto TCP 8083 solo en 127.0.0.1 por defecto. PostgreSQL,
Mosquitto y FastAPI permanecen en la red interna de Docker.

No usar activos reales, hardware físico ni exponer a Internet: el laboratorio
conserva autenticación y MQTT inseguros. La red LAN privada solo se habilita
explícitamente con `LOCKER_BIND_ADDRESS` y firewall restringido; debe probarse
desde un dispositivo autorizado antes de afirmar que funciona.

El perfil opcional `rust` escucha en `127.0.0.1:18001` y usa su propio
PostgreSQL temporal, sin conectarse a MQTT ni a la base del laboratorio. Al
detener `rust-postgres` se pierden sus datos temporales. Consulte el README
para comandos y configuración.
