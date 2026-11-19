# Plan maestro de migración a Rust — LOCKER

LOCKER sigue siendo un laboratorio aislado de préstamos, retiros, devoluciones y custodia de herramientas. El objetivo es migrar el backend a Rust, mantener y mejorar sus funciones, y optimizar recursos, conservando PostgreSQL, MQTT y, inicialmente, React/TypeScript. El diseño objetivo es un monolito modular; Axum, Tokio y SQLx son candidatos prioritarios.

## Condiciones del laboratorio

- Mantener `admin` / `admin` funcionando en todas las fases del laboratorio. No exigir MFA, OAuth, LDAP, rotación, recuperación de contraseñas ni sistemas avanzados de sesiones durante la migración simulada.
- Estas credenciales no son seguras para producción. No exponer el laboratorio públicamente ni conectar activos reales o controladores físicos mientras persistan sus riesgos.
- Autenticación insuficiente, contraseñas en texto claro y expuestas, MQTT anónimo y ausencia de TLS y autorización por tópico son **riesgos P0 aceptados temporalmente en laboratorio**; no se consideran resueltos. Los detalles y demás hallazgos se conservan en [RESUMEN_AUDITORIA.md](RESUMEN_AUDITORIA.md).
- La fase 5 es requisito antes de cualquier exposición pública, control físico o uso con activos reales. No bloquea el desarrollo con simulación aislada.
- Transacciones, idempotencia, consistencia de estados y prevención de duplicados son requisitos funcionales de la migración. Una apertura simulada no demuestra por sí sola el movimiento o cierre físico.
- Ningún par de componentes puede procesar simultáneamente la misma operación: durante el cambio, detener solicitudes, conciliar operaciones inciertas, desconectar al consumidor anterior y verificarlo antes de habilitar el nuevo. Aplicar el mismo criterio al revertir; no reenviar aperturas inciertas automáticamente.

## Fases y criterios

| Fase | Alcance y dependencias | Criterios de aceptación |
|---|---|---|
| **0 — Laboratorio y caracterización** | Seed explícito, arranque sin seed, PostgreSQL y MQTT aislados, pruebas de préstamo, devolución y reinicio de proceso. | Reinicio real conserva los datos simulados y no duplica registros; seed no automático; pruebas, Ruff, mypy y frontend pasan; plan actualizado. No se exige inspeccionar bases históricas, copia ni restauración. |
| **1 — Infraestructura Rust** | Tras fase 0, crear `apps/api-rust`, configuración, salud, compilación y CI, con Axum/Tokio/SQLx. Python sigue atendiendo el laboratorio. | Rust arranca aisladamente; pruebas y compilación pasan; datos y contratos existentes no cambian. |
| **2 — Dominio funcional** | Migrar administración, inventario, autorizaciones, préstamos y devoluciones por módulos; conservar PostgreSQL y los datos. Mantener acceso `admin` / `admin` de laboratorio. | Pruebas de paridad funcional, transacciones, concurrencia, reservas e idempotencia; sin préstamos ni ubicaciones duplicados. |
| **3 — MQTT, simulador y operaciones** | Migrar el puente y las operaciones simuladas; conservar MQTT, correlación y un único propietario activo. Incorporar recuperación y consistencia de estados. | Simulador completa flujos; probar desconexión, duplicados, mensajes tardíos, transferencia y reversión sin dos consumidores de la misma operación. |
| **4 — Frontend, optimización y retirada de Python** | Mantener React/TypeScript, corregir el historial y reducir cargas innecesarias; medir recursos y latencia con la misma carga. Retirar Python solo tras verificar paridad. | Flujos simulados completos, pruebas y CI pasan; comparación medida; procedimiento de reversión compatible con los datos. |
| **5 — Seguridad y preparación futura** | Revisar autenticación, contraseñas, autorización, TLS, MQTT por dispositivo, auditoría y protocolos físicos antes de producción. Definir un plan de credenciales de producción separado del laboratorio. | Análisis de amenazas y pruebas de acceso y dispositivos; validación física y operativa antes de activos reales o exposición pública. |

## Matriz de hallazgos de auditoría

| Nº | Tratamiento | Fase |
|---:|---|---:|
| 1 | Autenticación ausente en rutas: riesgo temporal aceptado en laboratorio; endurecer antes de producción. | 5 |
| 2 | Contraseñas en texto claro y expuestas: mantener credenciales de laboratorio; corregir para producción. | 5 |
| 3 | MQTT anónimo y sin identidad verificada: mantener aislado; asegurar antes de hardware real. | 5 |
| 4 | Seed automático: corregir y probar reinicio. | 0 |
| 5 | Apertura no prueba entrega: mantener distinción y caracterizar simulación; evidencia física antes de activos reales. | 2–3, 5 |
| 6 | Envío no durable: transacción, recuperación y pruebas de fallos. | 2–3 |
| 7 | Retiros concurrentes: transacciones e idempotencia. | 2 |
| 8 | Jerarquía y ocupación: validar relaciones y reservar compartimiento. | 2 |
| 9 | Identidad de locker ambigua: identificador global en protocolo simulado. | 3 |
| 10 | Actor de auditoría ausente: conservar trazabilidad funcional; atribución confiable en endurecimiento. | 2, 5 |
| 11 | CRUD sin validación: validar contratos y estados durante migración. | 2 |
| 12 | Identidad MQTT compartida: un solo procesador y transferencia controlada. | 3 |
| 13 | Consultas completas: paginar y cargar por vista tras medición. | 4 |
| 14 | Filtro de historial incorrecto: corregir y probar con dos activos. | 4 |
| 15 | Presencia sin evidencia: procesar latidos y mostrar estado real. | 3–4 |
| 16 | Falta de pruebas: caracterización y regresión por fase. | 0–5 |
| 17 | Fallos sin recuperación: estados y conciliación. | 2–3 |
| 18 | Esquemas sin uso: reutilizar lo necesario y retirar el resto con Python. | 2, 4 |

## Validaciones y límites

Las pruebas de Fase 0 utilizan exclusivamente datos y servicios aislados. La inspección de bases LOCKER anteriores, las copias y su restauración están fuera del alcance actual y no bloquean el desarrollo simulado. No modificar automáticamente datos existentes. La preparación de una eventual migración de datos reales se planificará antes de usar dichos datos.

En cada fase conservar contratos y datos funcionales, probar fallos y medir CPU, memoria, latencia y red antes de afirmar mejoras. Una futura PWA consumirá los mismos servicios; no forma parte de la fase 0.

## Avance de Fase 1

`apps/api-rust` contiene configuración, pool PostgreSQL y endpoints de salud con
Axum/Tokio/SQLx; no implementa préstamos ni MQTT. El perfil Docker opcional usa
un PostgreSQL distinto, temporal y sin migraciones ni seed. Python mantiene las
operaciones simuladas. Se validaron conexión disponible e indisponible, parada
ordenada y funcionamiento independiente del laboratorio. El acceso LAN desde
otro equipo no se ha validado y permanece deshabilitado por defecto.

## Cierre de los tres hallazgos de Fase 0

1. **Exposición del panel:** Compose publica `127.0.0.1:8083` por defecto. `LOCKER_BIND_ADDRESS` permite seleccionar explícitamente una dirección privada; la configuración efectiva se comprobó tanto con el valor predeterminado como con una dirección LAN de ejemplo. El acceso LAN requiere restringir la red y no convierte las credenciales de laboratorio en seguras.
2. **Sincronización MQTT:** la API marca la suscripción activa al recibir SUBACK y `/health/ready` responde 503 hasta entonces. El simulador anuncia disponibilidad después del SUBACK y usa aviso retenido de desconexión; las pruebas esperan una señal reciente con timeout y diagnóstico. Se probaron aceptación, rechazo y desconexión de suscripciones.
3. **Reinicio real Docker:** `python scripts/test_docker_restart.py` crea un proyecto Compose aislado con el Dockerfile normal, verifica arranque sin seed y login después del seed explícito, crea un préstamo, reinicia el contenedor y compara exactamente préstamos, ubicaciones y conteos de todas las tablas. Después ejecuta devolución, nuevo retiro y devolución. Sus recursos temporales se eliminan al terminar.

Validación ejecutada: integración API/PostgreSQL/MQTT 7 pruebas; simulador 4 pruebas; escenario de reinicio Docker aprobado; Ruff y mypy de API, Ruff de simulador, lint/build/test web (1 prueba), comprobación de Compose y `git diff --check` satisfactorios. La integración incluye avisos de deprecación existentes de FastAPI/Starlette. No se han probado hardware ni datos históricos.
