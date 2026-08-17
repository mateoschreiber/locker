# ADR-003: MQTT como frontera de hardware

## Decisión

Usar MQTT y un contrato JSON versionado para comandos, eventos y estado del
locker.

## Consecuencia

El laboratorio y un controlador físico podrán intercambiarse sin que el
dominio dependa de un proveedor de hardware.
