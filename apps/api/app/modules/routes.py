from uuid import UUID

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.db.session import get_db
from app.modules.models import (
    AuditEvent,
    Authorization,
    Branch,
    Camera,
    Compartment,
    Loan,
    Locker,
    LockerLock,
    Membership,
    Role,
    Tool,
    ToolPlacement,
    User,
)
from app.modules.schemas import (
    BranchCreate,
    LoginRequest,
    MembershipCreate,
    PlacementCreate,
    UserCreate,
)

router = APIRouter(prefix="/api/v1")


def record(session: Session, action: str, entity: object) -> None:
    session.add(
        AuditEvent(
            action=action,
            entity_type=entity.__class__.__name__,
            entity_id=str(getattr(entity, "id")),
            metadata_json={},
        )
    )


def dump(item: object) -> dict[str, object]:
    result: dict[str, object] = {}
    for column in item.__table__.columns:  # type: ignore[attr-defined]
        value = getattr(item, column.name)
        result[column.name] = str(value) if isinstance(value, UUID) else value
    return result


def list_items(session: Session, model: type[object]) -> list[dict[str, object]]:
    return [dump(item) for item in session.scalars(select(model).order_by(model.created_at)).all()]  # type: ignore[attr-defined]


@router.post("/auth/login", tags=["auth"])
def login(payload: LoginRequest, session: Session = Depends(get_db)) -> dict[str, object]:
    user = session.scalar(select(User).where(User.username == payload.username))
    if user is None or user.password != payload.password:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid credentials")
    record(session, "LOGIN", user)
    session.commit()
    return {"access_token": "lab-admin-token", "token_type": "bearer", "user": dump(user)}


@router.get("/auth/me", tags=["auth"])
def current_user(session: Session = Depends(get_db)) -> dict[str, object]:
    user = session.scalar(select(User).where(User.username == "admin"))
    if user is None:
        raise HTTPException(status_code=404, detail="Laboratory admin not seeded")
    return dump(user)


@router.get("/branches", tags=["branches"])
def branches(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Branch)


@router.post("/branches", status_code=status.HTTP_201_CREATED, tags=["branches"])
def create_branch(payload: BranchCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    item = Branch(**payload.model_dump())
    session.add(item)
    session.flush()
    record(session, "BRANCH_CREATED", item)
    session.commit()
    return dump(item)


@router.get("/users", tags=["users"])
def users(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, User)


@router.post("/users", status_code=status.HTTP_201_CREATED, tags=["users"])
def create_user(payload: UserCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    item = User(**payload.model_dump())
    session.add(item)
    session.flush()
    record(session, "USER_CREATED", item)
    session.commit()
    return dump(item)


@router.get("/memberships", tags=["users"])
def memberships(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Membership)


@router.post("/memberships", status_code=status.HTTP_201_CREATED, tags=["users"])
def create_membership(payload: MembershipCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    role = session.scalar(select(Role).where(Role.code == payload.role_code))
    if role is None:
        raise HTTPException(status_code=404, detail="Role not found")
    item = Membership(user_id=payload.user_id, branch_id=payload.branch_id, role_id=role.id)
    session.add(item)
    session.flush()
    record(session, "MEMBERSHIP_CREATED", item)
    session.commit()
    return dump(item)


def add_routes(path: str, model: type[object], action: str) -> None:
    @router.get(path, tags=[path.strip("/")])
    def get_all(session: Session = Depends(get_db)) -> list[dict[str, object]]:
        return list_items(session, model)

    @router.post(path, status_code=status.HTTP_201_CREATED, tags=[path.strip("/")])
    def create(payload: dict[str, object], session: Session = Depends(get_db)) -> dict[str, object]:
        item = model(**payload)  # type: ignore[call-arg]
        session.add(item)
        session.flush()
        record(session, action, item)
        session.commit()
        return dump(item)


add_routes("/lockers", Locker, "LOCKER_CREATED")
add_routes("/compartments", Compartment, "COMPARTMENT_CREATED")
add_routes("/locks", LockerLock, "LOCK_CREATED")
add_routes("/cameras", Camera, "CAMERA_CREATED")
add_routes("/tools", Tool, "TOOL_CREATED")


@router.get("/placements", tags=["inventory"])
def placements(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return [dump(item) for item in session.scalars(select(ToolPlacement).order_by(ToolPlacement.placed_at)).all()]


@router.post("/placements", status_code=status.HTTP_201_CREATED, tags=["inventory"])
def create_placement(payload: PlacementCreate, session: Session = Depends(get_db)) -> dict[str, object]:
    current_tool = session.scalar(
        select(ToolPlacement).where(ToolPlacement.tool_id == payload.tool_id, ToolPlacement.removed_at.is_(None))
    )
    current_compartment = session.scalar(
        select(ToolPlacement).where(
            ToolPlacement.compartment_id == payload.compartment_id, ToolPlacement.removed_at.is_(None)
        )
    )
    if current_tool is not None or current_compartment is not None:
        raise HTTPException(status_code=409, detail="Tool or compartment already has an active placement")
    item = ToolPlacement(**payload.model_dump())
    session.add(item)
    session.flush()
    record(session, "TOOL_PLACED", item)
    session.commit()
    return dump(item)


@router.get("/tools/{tool_id}/history", tags=["inventory"])
def tool_history(tool_id: UUID, session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return [
        dump(item)
        for item in session.scalars(
            select(ToolPlacement).where(ToolPlacement.tool_id == tool_id).order_by(ToolPlacement.placed_at)
        ).all()
    ]


@router.get("/authorizations", tags=["authorizations"])
def authorizations(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Authorization)


@router.get("/loans", tags=["loans"])
def loans(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, Loan)


@router.get("/audit", tags=["audit"])
def audit(session: Session = Depends(get_db)) -> list[dict[str, object]]:
    return list_items(session, AuditEvent)
