"""Operational simulated locker flow.

Revision ID: 20260817_0003
Revises: 20260817_0002
Create Date: 2026-08-17
"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "20260817_0003"
down_revision: str | None = "20260817_0002"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column("authorizations", sa.Column("approved_at", sa.DateTime(timezone=True), nullable=True))
    op.add_column("loans", sa.Column("checked_out_at", sa.DateTime(timezone=True), nullable=True))
    op.add_column("loans", sa.Column("returned_at", sa.DateTime(timezone=True), nullable=True))
    op.create_table(
        "locker_operations",
        sa.Column("id", sa.UUID(), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), server_default=sa.text("now()"), nullable=False),
        sa.Column("updated_at", sa.DateTime(timezone=True), server_default=sa.text("now()"), nullable=False),
        sa.Column("correlation_id", sa.String(80), nullable=False),
        sa.Column("command_type", sa.String(32), nullable=False),
        sa.Column("status", sa.String(32), nullable=False),
        sa.Column("tool_id", sa.UUID(), nullable=False),
        sa.Column("loan_id", sa.UUID(), nullable=False),
        sa.Column("branch_id", sa.UUID(), nullable=False),
        sa.Column("locker_id", sa.UUID(), nullable=False),
        sa.Column("compartment_id", sa.UUID(), nullable=False),
        sa.Column("confirmed_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column("failure_reason", sa.String(255), nullable=True),
        sa.ForeignKeyConstraint(["tool_id"], ["tools.id"]),
        sa.ForeignKeyConstraint(["loan_id"], ["loans.id"]),
        sa.ForeignKeyConstraint(["branch_id"], ["branches.id"]),
        sa.ForeignKeyConstraint(["locker_id"], ["lockers.id"]),
        sa.ForeignKeyConstraint(["compartment_id"], ["compartments.id"]),
        sa.PrimaryKeyConstraint("id"),
        sa.UniqueConstraint("correlation_id"),
    )


def downgrade() -> None:
    op.drop_table("locker_operations")
    op.drop_column("loans", "returned_at")
    op.drop_column("loans", "checked_out_at")
    op.drop_column("authorizations", "approved_at")
