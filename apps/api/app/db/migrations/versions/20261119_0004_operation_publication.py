"""Operation publication persistence (Fase 3 MQTT).

Revision ID: 20261119_0004
Revises: 20260817_0003
Create Date: 2026-11-19
"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "20261119_0004"
down_revision: str | None = "20260817_0003"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    # `origin` partitions operation ownership: rows created by the Python API
    # default to 'python' so the Rust bridge never claims them, even if both
    # backends are ever pointed at the same mutable database (audit #12).
    op.add_column(
        "locker_operations",
        sa.Column(
            "origin",
            sa.String(16),
            nullable=False,
            server_default=sa.text("'python'"),
        ),
    )
    op.add_column(
        "locker_operations",
        sa.Column("published_at", sa.DateTime(timezone=True), nullable=True),
    )


def downgrade() -> None:
    op.drop_column("locker_operations", "published_at")
    op.drop_column("locker_operations", "origin")
