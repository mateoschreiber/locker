from uuid import UUID

from datetime import datetime

from pydantic import BaseModel, Field


class LoginRequest(BaseModel):
    username: str
    password: str


class BranchCreate(BaseModel):
    code: str
    name: str


class UserCreate(BaseModel):
    username: str
    display_name: str
    password: str = "admin"


class MembershipCreate(BaseModel):
    user_id: UUID
    branch_id: UUID
    role_code: str


class LockerCreate(BaseModel):
    branch_id: UUID
    code: str
    name: str


class CompartmentCreate(BaseModel):
    locker_id: UUID
    code: str
    name: str
    position: int = Field(ge=1)


class LockCreate(BaseModel):
    compartment_id: UUID
    hardware_address: str | None = None


class CameraCreate(BaseModel):
    locker_id: UUID
    name: str


class ToolCreate(BaseModel):
    asset_code: str
    name: str
    description: str | None = None
    rfid_tag: str | None = None


class PlacementCreate(BaseModel):
    tool_id: UUID
    branch_id: UUID
    locker_id: UUID
    compartment_id: UUID


class StatusUpdate(BaseModel):
    status: str


class AuthorizationCreate(BaseModel):
    tool_id: UUID
    user_id: UUID
    branch_id: UUID


class OperationStart(BaseModel):
    authorization_id: UUID | None = None
    loan_id: UUID | None = None


class Page(BaseModel):
    items: list[dict[str, object]]
    total: int
    page: int
    page_size: int


class OperationResponse(BaseModel):
    id: UUID
    correlation_id: str
    command_type: str
    status: str
    loan_id: UUID
    created_at: datetime
