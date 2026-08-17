"""Domain foundation for the laboratory.

Revision ID: 20260817_0002
Revises: 20260817_0001
Create Date: 2026-08-17
"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "20260817_0002"
down_revision: str | None = "20260817_0001"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def audit_columns() -> list[sa.Column[object]]:
    return [
        sa.Column("id", sa.UUID(), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), server_default=sa.text("now()"), nullable=False),
        sa.Column("updated_at", sa.DateTime(timezone=True), server_default=sa.text("now()"), nullable=False),
    ]


def upgrade() -> None:
    op.create_table("branches", *audit_columns(), sa.Column("code", sa.String(64), nullable=False), sa.Column("name", sa.String(160), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("code"))
    op.create_table("roles", *audit_columns(), sa.Column("code", sa.String(32), nullable=False), sa.Column("name", sa.String(100), nullable=False), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("code"))
    op.create_table("users", *audit_columns(), sa.Column("username", sa.String(80), nullable=False), sa.Column("display_name", sa.String(160), nullable=False), sa.Column("password", sa.String(255), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("username"))
    op.create_table("memberships", *audit_columns(), sa.Column("user_id", sa.UUID(), nullable=False), sa.Column("branch_id", sa.UUID(), nullable=False), sa.Column("role_id", sa.UUID(), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["branch_id"], ["branches.id"]), sa.ForeignKeyConstraint(["role_id"], ["roles.id"]), sa.ForeignKeyConstraint(["user_id"], ["users.id"]), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("user_id", "branch_id", "role_id"))
    op.create_table("lockers", *audit_columns(), sa.Column("branch_id", sa.UUID(), nullable=False), sa.Column("code", sa.String(64), nullable=False), sa.Column("name", sa.String(160), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["branch_id"], ["branches.id"]), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("branch_id", "code"))
    op.create_table("compartments", *audit_columns(), sa.Column("locker_id", sa.UUID(), nullable=False), sa.Column("code", sa.String(64), nullable=False), sa.Column("name", sa.String(160), nullable=False), sa.Column("position", sa.Integer(), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["locker_id"], ["lockers.id"]), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("locker_id", "code"))
    op.create_table("locker_locks", *audit_columns(), sa.Column("compartment_id", sa.UUID(), nullable=False), sa.Column("hardware_address", sa.String(120)), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["compartment_id"], ["compartments.id"]), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("compartment_id"))
    op.create_table("cameras", *audit_columns(), sa.Column("locker_id", sa.UUID(), nullable=False), sa.Column("name", sa.String(160), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["locker_id"], ["lockers.id"]), sa.PrimaryKeyConstraint("id"))
    op.create_table("tools", *audit_columns(), sa.Column("asset_code", sa.String(80), nullable=False), sa.Column("name", sa.String(160), nullable=False), sa.Column("description", sa.Text()), sa.Column("rfid_tag", sa.String(120)), sa.Column("status", sa.String(32), nullable=False), sa.PrimaryKeyConstraint("id"), sa.UniqueConstraint("asset_code"), sa.UniqueConstraint("rfid_tag"))
    op.create_table("tool_placements", *audit_columns(), sa.Column("tool_id", sa.UUID(), nullable=False), sa.Column("branch_id", sa.UUID(), nullable=False), sa.Column("locker_id", sa.UUID(), nullable=False), sa.Column("compartment_id", sa.UUID(), nullable=False), sa.Column("placed_at", sa.DateTime(timezone=True), server_default=sa.text("now()"), nullable=False), sa.Column("removed_at", sa.DateTime(timezone=True)), sa.Column("reason", sa.String(80), nullable=False), sa.ForeignKeyConstraint(["branch_id"], ["branches.id"]), sa.ForeignKeyConstraint(["compartment_id"], ["compartments.id"]), sa.ForeignKeyConstraint(["locker_id"], ["lockers.id"]), sa.ForeignKeyConstraint(["tool_id"], ["tools.id"]), sa.PrimaryKeyConstraint("id"))
    op.create_table("authorizations", *audit_columns(), sa.Column("tool_id", sa.UUID(), nullable=False), sa.Column("user_id", sa.UUID(), nullable=False), sa.Column("branch_id", sa.UUID(), nullable=False), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["branch_id"], ["branches.id"]), sa.ForeignKeyConstraint(["tool_id"], ["tools.id"]), sa.ForeignKeyConstraint(["user_id"], ["users.id"]), sa.PrimaryKeyConstraint("id"))
    op.create_table("loans", *audit_columns(), sa.Column("tool_id", sa.UUID(), nullable=False), sa.Column("user_id", sa.UUID(), nullable=False), sa.Column("authorization_id", sa.UUID()), sa.Column("status", sa.String(32), nullable=False), sa.ForeignKeyConstraint(["authorization_id"], ["authorizations.id"]), sa.ForeignKeyConstraint(["tool_id"], ["tools.id"]), sa.ForeignKeyConstraint(["user_id"], ["users.id"]), sa.PrimaryKeyConstraint("id"))
    op.create_table("audit_events", *audit_columns(), sa.Column("action", sa.String(100), nullable=False), sa.Column("entity_type", sa.String(80), nullable=False), sa.Column("entity_id", sa.String(80), nullable=False), sa.Column("actor_id", sa.UUID()), sa.Column("metadata_json", sa.JSON(), nullable=False), sa.ForeignKeyConstraint(["actor_id"], ["users.id"]), sa.PrimaryKeyConstraint("id"))
    op.create_index("one_active_tool_placement", "tool_placements", ["tool_id"], unique=True, postgresql_where=sa.text("removed_at IS NULL"))
    op.create_index("one_active_compartment_placement", "tool_placements", ["compartment_id"], unique=True, postgresql_where=sa.text("removed_at IS NULL"))


def downgrade() -> None:
    for table in ("audit_events", "loans", "authorizations", "tool_placements", "tools", "cameras", "locker_locks", "compartments", "lockers", "memberships", "users", "roles", "branches"):
        op.drop_table(table)
