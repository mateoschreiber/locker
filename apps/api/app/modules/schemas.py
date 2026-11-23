from uuid import UUID

from pydantic import BaseModel


class LoginRequest(BaseModel):
    username: str
    password: str


class PlacementCreate(BaseModel):
    tool_id: UUID
    branch_id: UUID
    locker_id: UUID
    compartment_id: UUID


class AuthorizationCreate(BaseModel):
    tool_id: UUID
    user_id: UUID
    branch_id: UUID
