# Reglas del proyecto

- Trabajar siempre en `main`; no crear ramas nuevas ni hacer push automático.
- Conservar el dominio de préstamos, devoluciones y custodia de herramientas y equipos.
- El backend oficial es `apps/api-rust` (Rust / Axum / Tokio / SQLx); Python solo se utiliza en `apps/locker-simulator`.
- Actualizar `README.md` y `CHANGELOG.md` ante cambios de funcionalidades, configuración, requisitos, contratos o variables de entorno.
- Preferir soluciones simples y eficientes (principios Ponytail); ejecutar `.\scripts\check-local.ps1` y usar Docker cuando corresponda.
- Mantener `admin:admin` exclusivamente en el laboratorio hasta completar el endurecimiento de seguridad de producción (Fase 5). No publicar en Internet ni conectar hardware físico real mientras las credenciales de laboratorio estén activas.
