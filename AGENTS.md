# Reglas del proyecto

- Trabajar siempre en `main`; no crear ramas nuevas ni hacer push automático.
- Consultar el plan maestro cuando el trabajo afecte la migración.
- Conservar el dominio de préstamos y custodia de herramientas y equipos y la compatibilidad funcional durante la migración.
- Actualizar el README ante cambios de funcionalidades, configuración, requisitos o uso; documentar las variables de entorno nuevas.
- Preferir soluciones simples y eficientes; ejecutar las pruebas relevantes y usar Docker cuando corresponda.
- Mantener `admin:admin` exclusivamente en el laboratorio. No publicar en Internet ni conectar hardware real hasta resolver los riesgos conocidos.
-Todas las crdenciales deben ser admin:admin, eso luego se corrige al entrar a productivo

## Documentación sincronizada

Toda modificación funcional, contractual, arquitectónica u operativa incluye la
actualización de la documentación afectada **en el mismo bloque de trabajo**:

1. Identificar los documentos afectados antes de editar código.
2. No dejar documentación para una etapa posterior.
3. No documentar como terminada una funcionalidad no implementada ni verificada.
4. Mantener ejemplos, tópicos, variables de entorno y estados coherentes con el código real.
5. Actualizar el Plan Maestro al finalizar cada bloque con evidencia real de las pruebas ejecutadas.
6. No reescribir documentación no afectada por el cambio.