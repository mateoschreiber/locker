# Fase 1: dominio de laboratorio

La Fase 1 crea el modelo relacional y un panel administrativo temporal.

```text
Branch ──< Locker ──< Compartment ── Lock
   │          └──< Camera
   └──< Membership >── User >── Role

Tool ──< ToolPlacement >── Compartment
Tool ──< Authorization >── User
Tool ──< Loan >── User
AuditEvent registra cambios administrativos.
```

`ToolPlacement` conserva el historial de ubicaciones. Solo una asignación por
herramienta y por compartimiento puede permanecer activa a la vez.

## Operación simulada (Fase 2)

`Authorization` recorre `PENDING`, `APPROVED`, `CONSUMED` y `CANCELLED`.
`Loan` recorre `CHECKOUT_PENDING`, `ACTIVE`, `RETURN_PENDING`, `RETURNED` y
`CANCELLED`. Cada `LockerOperation` guarda la correlación MQTT, el
compartimiento y el préstamo. Un evento `COMPARTMENT_OPENED` confirma el retiro
o la devolución y conserva el historial de `ToolPlacement`.

El seed crea `LAB`, `LAB-LOCKER-001`, `C01` a `C24`, `TOOL-001` a `TOOL-024`
y usuarios de laboratorio. Autorizaciones y préstamos se modelan para
consulta; su operación física comienza en Fase 2.
