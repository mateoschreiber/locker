"""Corrección A (Fase 3): the 0004 publication migration must apply
incrementally on an existing 0003 database, preserve its rows and downgrade
cleanly. Runs against its own throwaway database inside the isolated suite."""

import os
import uuid

import pytest
import sqlalchemy as sa
from alembic import command
from alembic.config import Config

pytestmark = pytest.mark.skipif(
    os.getenv("LOCKER_ISOLATED_TEST") != "1", reason="requires isolated PostgreSQL"
)

MIGRATION_DB = "locker_migration_test"

# Minimal pre-0004 database with one PENDING operation.
BASE_ROWS = """
INSERT INTO branches (id, code, name, status) VALUES ('{branch}', 'MIG', 'Sucursal', 'ACTIVE');
INSERT INTO roles (id, code, name) VALUES ('{role}', 'TECH', 'Técnico');
INSERT INTO users (id, username, display_name, password, status)
    VALUES ('{user}', 'mig-user', 'Mig', 'admin', 'ACTIVE');
INSERT INTO memberships (id, user_id, branch_id, role_id, status)
    VALUES ('{member}', '{user}', '{branch}', '{role}', 'ACTIVE');
INSERT INTO lockers (id, branch_id, code, name, status)
    VALUES ('{locker}', '{branch}', 'L1', 'Locker', 'ONLINE');
INSERT INTO compartments (id, locker_id, code, name, position, status)
    VALUES ('{compartment}', '{locker}', 'C', 'Compartimiento', 1, 'ACTIVE');
INSERT INTO tools (id, asset_code, name, status)
    VALUES ('{tool}', 'MIG-001', 'Herramienta', 'AVAILABLE');
INSERT INTO authorizations (id, tool_id, user_id, branch_id, status)
    VALUES ('{authorization}', '{tool}', '{user}', '{branch}', 'PENDING');
INSERT INTO loans (id, tool_id, user_id, authorization_id, status)
    VALUES ('{loan}', '{tool}', '{user}', '{authorization}', 'CHECKOUT_PENDING');
INSERT INTO locker_operations (id, correlation_id, command_type, status, tool_id,
    loan_id, branch_id, locker_id, compartment_id)
    VALUES ('{operation}', 'mig-correlation-1', 'CHECKOUT', 'PENDING', '{tool}',
    '{loan}', '{branch}', '{locker}', '{compartment}');
"""


def test_0004_upgrade_preserves_data_and_downgrades_cleanly(monkeypatch):
    from app.core.config import get_settings

    base_url = get_settings().database_url
    admin = sa.create_engine(base_url)
    with admin.connect().execution_options(isolation_level="AUTOCOMMIT") as conn:
        conn.execute(sa.text(f"DROP DATABASE IF EXISTS {MIGRATION_DB}"))
        conn.execute(sa.text(f"CREATE DATABASE {MIGRATION_DB}"))
    # env.py reads get_settings(); point it at the throwaway database.
    monkeypatch.setenv("DATABASE_URL", base_url.rsplit("/", 1)[0] + "/" + MIGRATION_DB)
    get_settings.cache_clear()
    config = Config("alembic.ini")
    # Created before the first migration so a failing upgrade can never leave
    # `engine` unbound in the finally cleanup; create_engine connects lazily.
    engine = sa.create_engine(get_settings().database_url)
    try:
        # Existing base at 0003 with live data.
        command.upgrade(config, "20260817_0003")
        ids = {
            key: str(uuid.uuid4())
            for key in (
                "branch", "role", "user", "member", "locker", "compartment",
                "tool", "authorization", "loan", "operation",
            )
        }
        with engine.begin() as conn:
            conn.execute(sa.text(BASE_ROWS.format(**ids)))

        # Incremental upgrade to 0004 preserves the row and backfills origin.
        command.upgrade(config, "head")
        with engine.begin() as conn:
            columns = {
                row[0]
                for row in conn.execute(
                    sa.text(
                        "SELECT column_name FROM information_schema.columns"
                        " WHERE table_name = 'locker_operations'"
                    )
                )
            }
            assert {"published_at", "origin"} <= columns
            row = conn.execute(
                sa.text(
                    "SELECT command_type, status, origin, published_at"
                    " FROM locker_operations WHERE correlation_id = 'mig-correlation-1'"
                )
            ).fetchone()
            assert row.command_type == "CHECKOUT"
            assert row.status == "PENDING"
            assert row.origin == "python"
            assert row.published_at is None
            conn.execute(
                sa.text("UPDATE locker_operations SET published_at = now()")
            )

        # Clean downgrade removes the columns and keeps the data.
        command.downgrade(config, "20260817_0003")
        with engine.begin() as conn:
            columns = {
                row[0]
                for row in conn.execute(
                    sa.text(
                        "SELECT column_name FROM information_schema.columns"
                        " WHERE table_name = 'locker_operations'"
                    )
                )
            }
            assert "published_at" not in columns
            assert "origin" not in columns
            count = conn.execute(sa.text("SELECT count(*) FROM locker_operations")).scalar()
            assert count == 1
    finally:
        engine.dispose()
        admin.dispose()
        # Restore the suite's real URL before any other test imports settings.
        monkeypatch.undo()
        get_settings.cache_clear()
        with admin.connect().execution_options(isolation_level="AUTOCOMMIT") as conn:
            conn.execute(sa.text(f"DROP DATABASE IF EXISTS {MIGRATION_DB}"))


def test_migration_error_is_not_masked_by_cleanup():
    """A failing alembic run must surface its own error, never a cleanup
    NameError raised by the finally block (Fase 3.6, hallazgo de revisión)."""
    config = Config("alembic.ini")
    with pytest.raises(Exception) as excinfo:
        command.upgrade(config, "20261119_never_existed")
    assert not isinstance(excinfo.value, (NameError, UnboundLocalError)), (
        "the cleanup masked the real migration error"
    )
