# ADR-002: PostgreSQL

## Decisión

Usar PostgreSQL como fuente de verdad relacional y Alembic para migraciones.

## Consecuencia

Las operaciones críticas futuras podrán ejecutarse con transacciones y
restricciones consistentes.
