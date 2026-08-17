# Despliegue de laboratorio

El stack de Fase 0 publica solo el puerto TCP 8083 en la LAN. PostgreSQL,
Mosquitto y FastAPI permanecen en la red interna de Docker.

Antes de usar datos reales se debe cambiar toda credencial de `.env`, situar
la aplicación detrás de HTTPS y restringir el firewall a las redes permitidas.
