import uuid
from datetime import datetime

from sqlalchemy import JSON, DateTime, ForeignKey, Integer, String, Text, UniqueConstraint, func
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Base(DeclarativeBase):
    pass


class IdMixin:
    id: Mapped[uuid.UUID] = mapped_column(primary_key=True, default=uuid.uuid4)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), onupdate=func.now()
    )


class Branch(IdMixin, Base):
    __tablename__ = "branches"
    code: Mapped[str] = mapped_column(String(64), unique=True)
    name: Mapped[str] = mapped_column(String(160))
    status: Mapped[str] = mapped_column(String(32), default="ACTIVE")


class Role(IdMixin, Base):
    __tablename__ = "roles"
    code: Mapped[str] = mapped_column(String(32), unique=True)
    name: Mapped[str] = mapped_column(String(100))


class User(IdMixin, Base):
    __tablename__ = "users"
    username: Mapped[str] = mapped_column(String(80), unique=True)
    display_name: Mapped[str] = mapped_column(String(160))
    password: Mapped[str] = mapped_column(String(255), default="admin")
    status: Mapped[str] = mapped_column(String(32), default="ACTIVE")


class Membership(IdMixin, Base):
    __tablename__ = "memberships"
    __table_args__ = (UniqueConstraint("user_id", "branch_id", "role_id"),)
    user_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("users.id"))
    branch_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("branches.id"))
    role_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("roles.id"))
    status: Mapped[str] = mapped_column(String(32), default="ACTIVE")


class Locker(IdMixin, Base):
    __tablename__ = "lockers"
    __table_args__ = (UniqueConstraint("branch_id", "code"),)
    branch_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("branches.id"))
    code: Mapped[str] = mapped_column(String(64))
    name: Mapped[str] = mapped_column(String(160))
    status: Mapped[str] = mapped_column(String(32), default="ONLINE")


class Compartment(IdMixin, Base):
    __tablename__ = "compartments"
    __table_args__ = (UniqueConstraint("locker_id", "code"),)
    locker_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("lockers.id"))
    code: Mapped[str] = mapped_column(String(64))
    name: Mapped[str] = mapped_column(String(160))
    position: Mapped[int] = mapped_column(Integer)
    status: Mapped[str] = mapped_column(String(32), default="ACTIVE")


class LockerLock(IdMixin, Base):
    __tablename__ = "locker_locks"
    compartment_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("compartments.id"), unique=True)
    hardware_address: Mapped[str | None] = mapped_column(String(120), nullable=True)
    status: Mapped[str] = mapped_column(String(32), default="LOCKED")


class Camera(IdMixin, Base):
    __tablename__ = "cameras"
    locker_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("lockers.id"))
    name: Mapped[str] = mapped_column(String(160))
    status: Mapped[str] = mapped_column(String(32), default="SIMULATED")


class Tool(IdMixin, Base):
    __tablename__ = "tools"
    asset_code: Mapped[str] = mapped_column(String(80), unique=True)
    name: Mapped[str] = mapped_column(String(160))
    description: Mapped[str | None] = mapped_column(Text, nullable=True)
    rfid_tag: Mapped[str | None] = mapped_column(String(120), unique=True, nullable=True)
    status: Mapped[str] = mapped_column(String(32), default="AVAILABLE")


class ToolPlacement(IdMixin, Base):
    __tablename__ = "tool_placements"
    tool_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("tools.id"))
    branch_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("branches.id"))
    locker_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("lockers.id"))
    compartment_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("compartments.id"))
    placed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    removed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    reason: Mapped[str] = mapped_column(String(80), default="INITIAL_ASSIGNMENT")


class Authorization(IdMixin, Base):
    __tablename__ = "authorizations"
    tool_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("tools.id"))
    user_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("users.id"))
    branch_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("branches.id"))
    status: Mapped[str] = mapped_column(String(32), default="PENDING")
    approved_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)


class Loan(IdMixin, Base):
    __tablename__ = "loans"
    tool_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("tools.id"))
    user_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("users.id"))
    authorization_id: Mapped[uuid.UUID | None] = mapped_column(
        ForeignKey("authorizations.id"), nullable=True
    )
    status: Mapped[str] = mapped_column(String(32), default="CHECKOUT_PENDING")
    checked_out_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    returned_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)


class LockerOperation(IdMixin, Base):
    __tablename__ = "locker_operations"
    correlation_id: Mapped[str] = mapped_column(String(80), unique=True)
    command_type: Mapped[str] = mapped_column(String(32))
    status: Mapped[str] = mapped_column(String(32), default="PENDING")
    tool_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("tools.id"))
    loan_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("loans.id"))
    branch_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("branches.id"))
    locker_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("lockers.id"))
    compartment_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("compartments.id"))
    confirmed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    failure_reason: Mapped[str | None] = mapped_column(String(255), nullable=True)


class AuditEvent(IdMixin, Base):
    __tablename__ = "audit_events"
    action: Mapped[str] = mapped_column(String(100))
    entity_type: Mapped[str] = mapped_column(String(80))
    entity_id: Mapped[str] = mapped_column(String(80))
    actor_id: Mapped[uuid.UUID | None] = mapped_column(ForeignKey("users.id"), nullable=True)
    metadata_json: Mapped[dict[str, str]] = mapped_column(JSON, default=dict)
