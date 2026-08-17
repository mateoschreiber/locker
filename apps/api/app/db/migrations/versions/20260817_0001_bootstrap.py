"""Bootstrap migration.

Revision ID: 20260817_0001
Revises:
Create Date: 2026-08-17
"""

from collections.abc import Sequence

revision: str = "20260817_0001"
down_revision: str | None = None
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    """Reserve the initial Alembic revision without domain tables."""


def downgrade() -> None:
    """No schema objects exist in the bootstrap revision."""
