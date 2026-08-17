# ADR-004: Adaptadores de hardware

## Decisión

La lógica de negocio invocará una interfaz de gateway, no GPIO ni SDKs de
fabricantes. El simulador es el primer adaptador.

## Consecuencia

Las integraciones futuras se implementarán como adaptadores y mantendrán los
contratos de dominio y MQTT.
