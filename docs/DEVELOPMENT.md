# Desarrollo local

1. Copie `.env.example` a `.env`.
2. Ejecute `docker compose up -d --build` desde la raíz.
3. Compruebe `docker compose ps` y `curl http://localhost:8083/health/ready`.

Las aplicaciones se validan en sus directorios respectivos. No se requiere
ninguna dependencia instalada en el host para levantar el laboratorio.
