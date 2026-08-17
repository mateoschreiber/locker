from datetime import UTC, datetime
from uuid import UUID, uuid4

from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy import func, or_, select
from sqlalchemy.orm import Session

from app.db.session import get_db
from app.modules.models import (
    AuditEvent, Authorization, Branch, Camera, Compartment, Loan, Locker, LockerLock,
    LockerOperation, Membership, Role, Tool, ToolPlacement, User,
)
from app.modules.mqtt import mqtt_bridge
from app.modules.schemas import AuthorizationCreate, LoginRequest, PlacementCreate

router = APIRouter(prefix="/api/v1")


def dump(item: object) -> dict[str, object]:
    result: dict[str, object] = {}
    for column in item.__table__.columns:  # type: ignore[attr-defined]
        value = getattr(item, column.name)
        if isinstance(value, UUID):
            result[column.name] = str(value)
        elif isinstance(value, datetime):
            result[column.name] = value.isoformat()
        else:
            result[column.name] = value
    return result


def audit(session: Session, action: str, entity: object) -> None:
    session.add(AuditEvent(action=action, entity_type=entity.__class__.__name__, entity_id=str(getattr(entity, "id")), metadata_json={}))


def list_items(session: Session, model: type[object], q: str | None = None, item_status: str | None = None) -> list[dict[str, object]]:
    query = select(model).order_by(model.created_at.desc())  # type: ignore[attr-defined]
    if item_status and hasattr(model, "status"):
        query = query.where(model.status == item_status)  # type: ignore[attr-defined]
    if q:
        fields = [getattr(model, field) for field in ("name", "code", "asset_code", "username", "display_name") if hasattr(model, field)]
        if fields:
            query = query.where(or_(*[field.ilike(f"%{q}%") for field in fields]))
    return [dump(item) for item in session.scalars(query).all()]


@router.post("/auth/login", tags=["auth"])
def login(payload: LoginRequest, session: Session = Depends(get_db)) -> dict[str, object]:
    user = session.scalar(select(User).where(User.username == payload.username))
    if user is None or user.password != payload.password:
        raise HTTPException(status_code=401, detail="Credenciales inválidas")
    audit(session, "LOGIN", user)
    session.commit()
    return {"access_token": "lab-admin-token", "token_type": "bearer", "user": dump(user)}


@router.get("/auth/me", tags=["auth"])
def current_user(session: Session = Depends(get_db)) -> dict[str, object]:
    user = session.scalar(select(User).where(User.username == "admin"))
    if user is None:
        raise HTTPException(status_code=404, detail="Administrador no encontrado")
    return dump(user)


def add_admin_routes(path: str, model: type[object], action: str) -> None:
    @router.get(path, tags=["administración"])
    def get_all(q: str | None = None, status: str | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
        return list_items(session, model, q, status)

    @router.get(f"{path}/{{item_id}}", tags=["administración"])
    def get_one(item_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
        item = session.get(model, item_id)
        if item is None:
            raise HTTPException(status_code=404, detail="Registro no encontrado")
        return dump(item)

    @router.post(path, status_code=status.HTTP_201_CREATED, tags=["administración"])
    def create(payload: dict[str, object], session: Session = Depends(get_db)) -> dict[str, object]:
        item = model(**payload)  # type: ignore[call-arg]
        session.add(item)
        session.flush()
        audit(session, f"{action}_CREATED", item)
        session.commit()
        return dump(item)

    @router.patch(f"{path}/{{item_id}}", tags=["administración"])
    def update(item_id: UUID, payload: dict[str, object], session: Session = Depends(get_db)) -> dict[str, object]:
        item = session.get(model, item_id)
        if item is None:
            raise HTTPException(status_code=404, detail="Registro no encontrado")
        for key, value in payload.items():
            if key not in {"id", "created_at", "updated_at"} and hasattr(item, key):
                setattr(item, key, value)
        audit(session, f"{action}_UPDATED", item)
        session.commit()
        return dump(item)

    @router.post(f"{path}/{{item_id}}/deactivate", tags=["administración"])
    def deactivate(item_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
        item = session.get(model, item_id)
        if item is None:
            raise HTTPException(status_code=404, detail="Registro no encontrado")
        if not hasattr(item, "status"):
            raise HTTPException(status_code=409, detail="La entidad no admite desactivación")
        item.status = "INACTIVE"
        audit(session, f"{action}_DEACTIVATED", item)
        session.commit()
        return dump(item)


for _path, _model, _action in (
    ("/branches", Branch, "BRANCH"), ("/roles", Role, "ROLE"), ("/users", User, "USER"),
    ("/memberships", Membership, "MEMBERSHIP"), ("/lockers", Locker, "LOCKER"),
    ("/compartments", Compartment, "COMPARTMENT"), ("/locks", LockerLock, "LOCK"),
    ("/cameras", Camera, "CAMERA"), ("/tools", Tool, "TOOL"),
):
    add_admin_routes(_path, _model, _action)


@router.get("/placements", tags=["inventario"])
def placements(tool_id: UUID | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    query = select(ToolPlacement).order_by(ToolPlacement.placed_at.desc())
    if tool_id:
        query = query.where(ToolPlacement.tool_id == tool_id)
    return [dump(item) for item in session.scalars(query).all()]


@router.post("/placements", status_code=201, tags=["inventario"])
def create_placement(payload: PlacementCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    current_tool = session.scalar(select(ToolPlacement).where(ToolPlacement.tool_id == payload.tool_id, ToolPlacement.removed_at.is_(None)))
    current_compartment = session.scalar(select(ToolPlacement).where(ToolPlacement.compartment_id == payload.compartment_id, ToolPlacement.removed_at.is_(None)))
    if current_tool or current_compartment:
        raise HTTPException(status_code=409, detail="Herramienta o compartimiento ya tiene una ubicación activa")
    item = ToolPlacement(**payload.model_dump())
    session.add(item)
    session.flush()
    audit(session, "TOOL_PLACED", item)
    session.commit()
    return dump(item)


@router.get("/tools/{tool_id}/history", tags=["inventario"])
def tool_history(tool_id: UUID, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return [dump(item) for item in session.scalars(select(ToolPlacement).where(ToolPlacement.tool_id == tool_id).order_by(ToolPlacement.placed_at.desc())).all()]


@router.get("/lockers/{locker_id}/detail", tags=["lockers"])
def locker_detail(locker_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
    locker = session.get(Locker, locker_id)
    if locker is None:
        raise HTTPException(status_code=404, detail="Locker no encontrado")
    compartments = session.scalars(select(Compartment).where(Compartment.locker_id == locker_id).order_by(Compartment.position)).all()
    active = {item.compartment_id: item for item in session.scalars(select(ToolPlacement).where(ToolPlacement.locker_id == locker_id, ToolPlacement.removed_at.is_(None))).all()}
    tools = {item.id: item for item in session.scalars(select(Tool)).all()}
    locks = {item.compartment_id: item for item in session.scalars(select(LockerLock).where(LockerLock.compartment_id.in_([c.id for c in compartments]))).all()} if compartments else {}
    return {"locker": dump(locker), "cameras": [dump(item) for item in session.scalars(select(Camera).where(Camera.locker_id == locker_id)).all()], "compartments": [{**dump(compartment), "tool": dump(tools[active[compartment.id].tool_id]) if compartment.id in active else None, "lock": dump(locks[compartment.id]) if compartment.id in locks else None} for compartment in compartments]}


@router.get("/authorizations", tags=["operación"])
def authorizations(status: str | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Authorization, item_status=status)


@router.post("/authorizations", status_code=201, tags=["operación"])
def create_authorization(payload: AuthorizationCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    item = Authorization(**payload.model_dump())
    session.add(item); session.flush(); audit(session, "AUTHORIZATION_CREATED", item); session.commit()
    return dump(item)


@router.post("/authorizations/{authorization_id}/approve", tags=["operación"])
def approve_authorization(authorization_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
    item = session.get(Authorization, authorization_id)
    if item is None or item.status != "PENDING":
        raise HTTPException(status_code=409, detail="La autorización no está pendiente")
    item.status, item.approved_at = "APPROVED", datetime.now(UTC)
    audit(session, "AUTHORIZATION_APPROVED", item); session.commit()
    return dump(item)


@router.post("/authorizations/{authorization_id}/cancel", tags=["operación"])
def cancel_authorization(authorization_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
    item = session.get(Authorization, authorization_id)
    if item is None or item.status not in {"PENDING", "APPROVED"}:
        raise HTTPException(status_code=409, detail="La autorización no puede cancelarse")
    item.status = "CANCELLED"; audit(session, "AUTHORIZATION_CANCELLED", item); session.commit()
    return dump(item)


def create_operation(session: Session, authorization: Authorization | None, loan: Loan | None, command_type: str) -> tuple[LockerOperation, Locker]:
    tool_id = authorization.tool_id if authorization else loan.tool_id  # type: ignore[union-attr]
    placement = session.scalar(select(ToolPlacement).where(ToolPlacement.tool_id == tool_id).order_by(ToolPlacement.placed_at.desc()))
    if placement is None:
        raise HTTPException(status_code=409, detail="No existe ubicación de origen para la herramienta")
    actual_loan = loan or Loan(tool_id=authorization.tool_id, user_id=authorization.user_id, authorization_id=authorization.id, status="CHECKOUT_PENDING")  # type: ignore[union-attr]
    if loan is None:
        session.add(actual_loan); session.flush()
        authorization.status = "CONSUMED"  # type: ignore[union-attr]
    else:
        actual_loan.status = "RETURN_PENDING"
    operation = LockerOperation(correlation_id=str(uuid4()), command_type=command_type, tool_id=tool_id, loan_id=actual_loan.id, branch_id=placement.branch_id, locker_id=placement.locker_id, compartment_id=placement.compartment_id)
    session.add(operation); session.flush(); audit(session, f"{command_type}_REQUESTED", operation)
    locker = session.get(Locker, placement.locker_id)
    if locker is None:
        raise HTTPException(status_code=409, detail="Locker no encontrado")
    return operation, locker


@router.post("/authorizations/{authorization_id}/checkout", status_code=202, tags=["operación"])
def checkout(authorization_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
    authorization = session.get(Authorization, authorization_id)
    if authorization is None or authorization.status != "APPROVED":
        raise HTTPException(status_code=409, detail="Se requiere una autorización aprobada")
    operation, locker = create_operation(session, authorization, None, "CHECKOUT")
    session.commit()
    mqtt_bridge.publish_open(locker.code, str(operation.compartment_id), operation.correlation_id)
    return dump(operation)


@router.get("/loans", tags=["operación"])
def loans(status: str | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Loan, item_status=status)


@router.post("/loans/{loan_id}/return", status_code=202, tags=["operación"])
def return_loan(loan_id: UUID, session: Session = Depends(get_db)) -> dict[str, object]:
    loan = session.get(Loan, loan_id)
    if loan is None or loan.status != "ACTIVE":
        raise HTTPException(status_code=409, detail="El préstamo no está activo")
    operation, locker = create_operation(session, None, loan, "RETURN")
    session.commit()
    mqtt_bridge.publish_open(locker.code, str(operation.compartment_id), operation.correlation_id)
    return dump(operation)


@router.get("/operations", tags=["operación"])
def operations(status: str | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, LockerOperation, item_status=status)


@router.get("/audit", tags=["actividad"])
def audit_events(q: str | None = None, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, AuditEvent, q=q)


@router.get("/dashboard", tags=["inicio"])
def dashboard(session: Session = Depends(get_db)) -> dict[str, object]:
    return {"lockers": session.scalar(select(func.count()).select_from(Locker)) or 0, "tools_available": session.scalar(select(func.count()).select_from(Tool).where(Tool.status == "AVAILABLE")) or 0, "tools_on_loan": session.scalar(select(func.count()).select_from(Tool).where(Tool.status == "ON_LOAN")) or 0, "pending_loans": session.scalar(select(func.count()).select_from(Loan).where(Loan.status.in_(["CHECKOUT_PENDING", "RETURN_PENDING"]))) or 0, "recent_activity": [dump(item) for item in session.scalars(select(AuditEvent).order_by(AuditEvent.created_at.desc()).limit(10)).all()]}
