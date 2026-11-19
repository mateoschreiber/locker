# Resumen de auditoría técnica — LOCKER

Fecha: 19 de noviembre de 2026. Revisión del repositorio en el commit `cb558b3`.

## Diagnóstico

LOCKER es un laboratorio para gestionar y simular la custodia y el préstamo de herramientas mediante lockers electrónicos. Dispone de un panel web, una API, PostgreSQL, MQTT y un simulador; no controla hardware físico. El análisis consideró su posible evolución hacia varias sucursales, dispositivos y operadores simultáneos.

**Madurez:** prototipo de laboratorio. No está preparado para custodiar activos reales ni para una migración segura a Rust sin resolver primero la seguridad, la integridad de los préstamos, la recuperación y las pruebas. El dominio debe seguir siendo el préstamo y custodia de herramientas, no paquetería.

## Arquitectura y funcionamiento actual

```text
Navegador → Nginx y React → FastAPI → PostgreSQL
                               ↕
                         MQTT / Mosquitto ↔ Simulador
```

- Hay modelos y operaciones administrativas para sucursales, usuarios, roles, lockers, compartimientos, herramientas y ubicaciones. Los roles existen como datos, pero no se aplican como permisos en las rutas revisadas.
- Una autorización aprobada inicia un retiro; se crea un préstamo pendiente y se publica una orden MQTT. Una devolución se inicia desde un préstamo activo.
- El simulador responde con un evento de apertura y la API cambia el estado del préstamo. Esta confirmación es simulada: no demuestra retiro, depósito ni cierre físico.
- Se guardan ubicaciones, préstamos, operaciones y eventos de auditoría, pero falta atribución confiable de actores y dispositivos. No se encontró una PWA implementada.
- PostgreSQL tiene índices únicos parciales para impedir más de una ubicación activa por herramienta y por compartimiento; no cubren todas las invariantes del préstamo.

## Hallazgos prioritarios

| Nº | Prioridad | Hallazgo y consecuencia | Acción recomendada | Evidencia |
|---:|:---:|---|---|---|
| 1 | P0 | Las rutas administrativas y operativas no validan una sesión ni permisos; el login devuelve un token fijo. El formulario no protege la API. | Implementar autenticación, expiración y revocación, y autorización por acción, rol y sucursal. | [Rutas](apps/api/app/modules/routes.py#L77), [cliente](apps/web/src/App.tsx#L12) |
| 2 | P0 | Las contraseñas se almacenan en texto claro y la serialización genérica puede devolverlas. El acceso HTTP tampoco cifra las credenciales. | Usar hashes adecuados, excluir credenciales de las respuestas, eliminar valores de laboratorio fuera de ese entorno y usar HTTPS. | [Modelo](apps/api/app/modules/models.py#L33), [serialización](apps/api/app/modules/routes.py#L36) |
| 3 | P0 | MQTT admite clientes anónimos; no se vincula la confirmación a una identidad de dispositivo verificada. Un participante con acceso al broker podría publicar órdenes o eventos. | Autenticar servicios y dispositivos, usar TLS y permisos por tópico, validar identidad y rechazar mensajes vencidos o repetidos. | [Mosquitto](infra/docker/mosquitto.conf#L3), [receptor MQTT](apps/api/app/modules/mqtt.py#L73) |
| 4 | P0 | El seed se ejecuta en cada arranque y puede volver a ubicar una herramienta prestada o impedir el inicio si el compartimiento ya está ocupado. | Ejecutar la carga ficticia explícitamente en una base de laboratorio nueva, no durante cada arranque. | [Arranque](apps/api/app/main.py#L20), [seed](apps/api/app/modules/seed.py#L65) |
| 5 | P0 | Una apertura simulada se trata como retiro o devolución completados, sin prueba de movimiento del activo ni cierre de puerta. | Separar orden, apertura, movimiento y cierre; incorporar evidencia física o confirmación operativa y conciliación. | [Confirmación](apps/api/app/modules/mqtt.py#L102), [simulador](apps/locker-simulator/locker_simulator/__main__.py#L62) |
| 6 | P1 | La operación se guarda antes de publicar MQTT, sin entrega durable, vencimiento ni recuperación. Una caída puede dejarla pendiente indefinidamente. | Registrar la intención de envío en la transacción y procesarla con recuperación, reintentos seguros y conciliación. | [Retiro](apps/api/app/modules/routes.py#L352), [publicación](apps/api/app/modules/mqtt.py#L53) |
| 7 | P1 | Solicitudes simultáneas pueden iniciar más de un retiro para una autorización o herramienta. | Proteger las transiciones con bloqueo o actualización condicional, restricciones de base e idempotencia. | [Operación](apps/api/app/modules/routes.py#L301) |
| 8 | P1 | Las relaciones de ubicación no comprueban toda la jerarquía sucursal–locker–compartimiento; la devolución puede ordenar la apertura antes de detectar ocupación. | Validar la jerarquía y reservar un compartimiento disponible antes de enviar la orden. | [Ubicaciones](apps/api/app/modules/routes.py#L169), [devolución](apps/api/app/modules/routes.py#L368) |
| 9 | P1 | El código de locker es único por sucursal, pero el tópico MQTT usa solo ese código. Dos lockers pueden compartir tópico. | Usar una identidad global e inmutable por dispositivo o imponer unicidad global coherente con el protocolo. | [Modelo](apps/api/app/modules/models.py#L50), [tópico](apps/api/app/modules/mqtt.py#L53) |
| 10 | P1 | Los eventos de auditoría no completan el actor en las acciones revisadas. | Registrar actor autenticado, dispositivo verificado, acción, resultado y correlación. | [Auditoría](apps/api/app/modules/routes.py#L49), [modelo](apps/api/app/modules/models.py#L138) |
| 11 | P1 | Las rutas administrativas genéricas aceptan campos y estados sin aplicar los esquemas de dominio específicos. | Usar esquemas y listas explícitas de campos editables; validar relaciones, estados y errores de integridad. | [Rutas administrativas](apps/api/app/modules/routes.py#L109), [esquemas](apps/api/app/modules/schemas.py#L12) |
| 12 | P1 | Varias instancias de la API usarían el mismo identificador MQTT y ejecutarían el seed, lo que complica la operación concurrente. | Coordinar el consumidor, definir identidades apropiadas y separar la inicialización del arranque de réplicas. | [Cliente MQTT](apps/api/app/modules/mqtt.py#L35), [arranque](apps/api/app/main.py#L20) |
| 13 | P2 | Cada panel solicita 14 colecciones y el dashboard cada cuatro segundos; las listas no están paginadas. | Paginar, cargar solo lo necesario por vista y medir antes de cambiar la tecnología de tiempo real. | [Carga web](apps/web/src/App.tsx#L8), [listados](apps/api/app/modules/routes.py#L60) |
| 14 | P2 | El historial de una herramienta puede incluir ubicaciones de otros activos por una condición de filtrado. | Filtrar cada ubicación por su herramienta y probar con al menos dos activos. | [Inventario](apps/web/src/App.tsx#L76) |
| 15 | P2 | El panel muestra conexión sin basarse en los latidos publicados por los dispositivos. | Consumir presencia, definir su vencimiento y mostrar el estado según evidencia. | [Panel](apps/web/src/App.tsx#L65), [simulador](apps/locker-simulator/locker_simulator/__main__.py#L22) |
| 16 | P2 | Las pruebas existentes no cubren permisos, préstamos completos, concurrencia, reinicios ni caída del broker. | Añadir pruebas de integración aisladas y pruebas de interfaz para los flujos críticos. | [API](apps/api/tests/test_routes.py), [web](apps/web/src/App.test.ts), [protocolo](apps/locker-simulator/tests/test_protocol.py) |
| 17 | P2 | Una confirmación fallida puede dejar el préstamo en estado pendiente sin una vía operativa de recuperación. | Definir transiciones de fallo y conciliación antes de reintentar o revertir una operación física. | [Confirmación](apps/api/app/modules/mqtt.py#L93) |
| 18 | P3 | Se encontraron 12 clases de esquema sin referencias fuera de su declaración, equivalentes a 48 líneas de clase. | Reutilizar las necesarias para validar contratos y eliminar solo las que sigan sin uso. | [Esquemas](apps/api/app/modules/schemas.py#L12) |

## Hardware y operación

MQTT es una frontera útil para mantener el backend independiente del fabricante. Para conectar cerraduras reales faltan identidad autenticada por dispositivo, correspondencia entre compartimiento y dirección física, confirmaciones de apertura y cierre, evidencia del movimiento, deduplicación, recuperación y conciliación. El campo `hardware_address` existe, pero no se usa en la orden MQTT revisada.

Los estados actuales sirven para el ensayo simulado; no representan todavía toda la operación física. El panel y los procedimientos deben distinguir una orden solicitada de un movimiento efectivamente confirmado.

## Rendimiento y frontend

Las 15 solicitudes por ciclo equivalen aproximadamente a 3,75 solicitudes por segundo por panel activo en condiciones normales: es un cálculo a partir del código, no una medición de carga. No se midieron CPU, memoria, latencia, consultas, tamaño del bundle ni impacto de Docker.

Se recomienda mantener React y TypeScript mientras se corrigen paginación, carga por vista y pruebas. Svelte, Vue y HTML servido con HTMX son alternativas viables para evaluar con un prototipo equivalente; ninguna demuestra por sí sola menor consumo en este proyecto. Una futura PWA requerirá diseñar expresamente sesión, conectividad y sincronización.

## Migración futura a Rust

Rust, Axum, Tokio y SQLx son candidatos viables, pero no se dispone de una línea base que permita prometer una mejora concreta. Conviene conservar PostgreSQL, MQTT, el simulador ampliado y los contratos REST y de eventos; reutilizar conceptualmente el dominio y sus casos de prueba; y reemplazar módulos Python de forma gradual solo después de comprobar equivalencia.

Durante la convivencia debe existir un único responsable de cada operación y consumidor MQTT. La migración debe contemplar transacciones, compatibilidad de contratos, reversión y mediciones comparables; cambiar de lenguaje no corrige automáticamente los defectos actuales.

## Orden recomendado

1. Aislar el laboratorio y retirar el seed automático del arranque normal.
2. Definir reglas de custodia, estados, reservas, evidencia física y conciliación.
3. Implementar autenticación, permisos, protección de contraseñas y seguridad MQTT.
4. Corregir concurrencia, idempotencia, entrega durable y recuperación ante fallos.
5. Probar los flujos con PostgreSQL y broker aislados, además de pruebas de interfaz.
6. Medir carga y recursos; optimizar consultas, paginación y actualización del panel.
7. Evaluar Rust con un piloto acotado y migrar por fases si los resultados lo justifican.
8. Integrar hardware real y validar sensores y procedimientos antes de usarlo para custodia.

## Alcance y límites de la verificación

Se revisaron los flujos críticos, configuración y pruebas del repositorio. Se analizaron sintácticamente 24 archivos Python y se ejecutaron directamente tres comprobaciones del protocolo, sin `pytest`. No se ejecutaron las suites completas, la compilación web, Compose integral, migraciones sobre PostgreSQL, CI, pruebas de carga o seguridad dinámicas, ni se validó hardware físico. El historial Git completo tampoco se auditó.

La auditoría fue de lectura: no se modificaron archivos del proyecto, bases de datos ni dependencias durante esa revisión. Este resumen se agregó posteriormente a petición del usuario.
