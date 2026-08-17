from sqlalchemy import select
from sqlalchemy.orm import Session

from app.modules.models import (
    Branch,
    Camera,
    Compartment,
    Locker,
    LockerLock,
    Membership,
    Role,
    Tool,
    ToolPlacement,
    User,
)


def seed_lab(session: Session) -> None:
    roles: dict[str, Role] = {}
    for code in ("ADMIN", "COORDINATOR", "MECHANIC"):
        role = session.scalar(select(Role).where(Role.code == code))
        if role is None:
            role = Role(code=code, name=code.title())
            session.add(role)
        roles[code] = role
    session.flush()

    branch = session.scalar(select(Branch).where(Branch.code == "LAB"))
    if branch is None:
        branch = Branch(code="LAB", name="Laboratorio")
        session.add(branch)
        session.flush()

    locker = session.scalar(select(Locker).where(Locker.code == "LAB-LOCKER-001"))
    if locker is None:
        locker = Locker(branch_id=branch.id, code="LAB-LOCKER-001", name="Locker de laboratorio")
        session.add(locker)
        session.flush()

    for position in range(1, 25):
        code = f"C{position:02d}"
        compartment = session.scalar(
            select(Compartment).where(Compartment.locker_id == locker.id, Compartment.code == code)
        )
        if compartment is None:
            compartment = Compartment(locker_id=locker.id, code=code, name=code, position=position)
            session.add(compartment)
            session.flush()
        if session.scalar(select(LockerLock).where(LockerLock.compartment_id == compartment.id)) is None:
            session.add(LockerLock(compartment_id=compartment.id, hardware_address=f"SIM-{code}"))

        asset_code = f"TOOL-{position:03d}"
        tool = session.scalar(select(Tool).where(Tool.asset_code == asset_code))
        if tool is None:
            tool = Tool(asset_code=asset_code, name=f"Herramienta {position:03d}", rfid_tag=f"RFID-{asset_code}")
            session.add(tool)
            session.flush()
        if session.scalar(select(ToolPlacement).where(ToolPlacement.tool_id == tool.id, ToolPlacement.removed_at.is_(None))) is None:
            session.add(
                ToolPlacement(
                    tool_id=tool.id,
                    branch_id=branch.id,
                    locker_id=locker.id,
                    compartment_id=compartment.id,
                )
            )

    if session.scalar(select(Camera).where(Camera.locker_id == locker.id)) is None:
        session.add(Camera(locker_id=locker.id, name="Cámara simulada"))

    users = (("admin", "Administrador", "ADMIN"), ("coordinator", "Coordinador", "COORDINATOR"), ("mechanic", "Mecánico", "MECHANIC"))
    for username, display_name, role_code in users:
        user = session.scalar(select(User).where(User.username == username))
        if user is None:
            user = User(username=username, display_name=display_name, password="admin")
            session.add(user)
            session.flush()
        if session.scalar(
            select(Membership).where(
                Membership.user_id == user.id,
                Membership.branch_id == branch.id,
                Membership.role_id == roles[role_code].id,
            )
        ) is None:
            session.add(Membership(user_id=user.id, branch_id=branch.id, role_id=roles[role_code].id))
    session.commit()
