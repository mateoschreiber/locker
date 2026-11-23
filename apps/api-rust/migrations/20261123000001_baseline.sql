-- SQLx baseline migration (F4.3/F4.4): captures the complete schema at
-- Alembic head 20261119_0004. Uses IF NOT EXISTS so running `migrate` on an
-- existing Alembic-created database preserves all data and records the
-- baseline in `_sqlx_migrations`, while an empty database gets the full schema.

CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
    success BOOLEAN NOT NULL
);

CREATE TABLE IF NOT EXISTS branches (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    code VARCHAR(64) NOT NULL,
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (code)
);

CREATE TABLE IF NOT EXISTS roles (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    code VARCHAR(32) NOT NULL,
    name VARCHAR(100) NOT NULL,
    UNIQUE (code)
);

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    username VARCHAR(80) NOT NULL,
    display_name VARCHAR(160) NOT NULL,
    password VARCHAR(255) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (username)
);

CREATE TABLE IF NOT EXISTS memberships (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_id UUID NOT NULL REFERENCES users (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    role_id UUID NOT NULL REFERENCES roles (id),
    status VARCHAR(32) NOT NULL,
    UNIQUE (user_id, branch_id, role_id)
);

CREATE TABLE IF NOT EXISTS lockers (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    branch_id UUID NOT NULL REFERENCES branches (id),
    code VARCHAR(64) NOT NULL,
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (branch_id, code)
);

CREATE TABLE IF NOT EXISTS compartments (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locker_id UUID NOT NULL REFERENCES lockers (id),
    code VARCHAR(64) NOT NULL,
    name VARCHAR(160) NOT NULL,
    position INTEGER NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (locker_id, code)
);

CREATE TABLE IF NOT EXISTS locker_locks (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    compartment_id UUID NOT NULL REFERENCES compartments (id),
    hardware_address VARCHAR(120),
    status VARCHAR(32) NOT NULL,
    UNIQUE (compartment_id)
);

CREATE TABLE IF NOT EXISTS cameras (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locker_id UUID NOT NULL REFERENCES lockers (id),
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL
);

CREATE TABLE IF NOT EXISTS tools (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    asset_code VARCHAR(80) NOT NULL,
    name VARCHAR(160) NOT NULL,
    description TEXT,
    rfid_tag VARCHAR(120),
    status VARCHAR(32) NOT NULL,
    UNIQUE (asset_code),
    UNIQUE (rfid_tag)
);

CREATE TABLE IF NOT EXISTS tool_placements (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    tool_id UUID NOT NULL REFERENCES tools (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    locker_id UUID NOT NULL REFERENCES lockers (id),
    compartment_id UUID NOT NULL REFERENCES compartments (id),
    placed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at TIMESTAMPTZ,
    reason VARCHAR(80) NOT NULL
);

CREATE TABLE IF NOT EXISTS authorizations (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    tool_id UUID NOT NULL REFERENCES tools (id),
    user_id UUID NOT NULL REFERENCES users (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    status VARCHAR(32) NOT NULL,
    approved_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS loans (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    tool_id UUID NOT NULL REFERENCES tools (id),
    user_id UUID NOT NULL REFERENCES users (id),
    authorization_id UUID REFERENCES authorizations (id),
    status VARCHAR(32) NOT NULL,
    checked_out_at TIMESTAMPTZ,
    returned_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS locker_operations (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    correlation_id VARCHAR(80) NOT NULL,
    command_type VARCHAR(32) NOT NULL,
    status VARCHAR(32) NOT NULL,
    tool_id UUID NOT NULL REFERENCES tools (id),
    loan_id UUID NOT NULL REFERENCES loans (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    locker_id UUID NOT NULL REFERENCES lockers (id),
    compartment_id UUID NOT NULL REFERENCES compartments (id),
    confirmed_at TIMESTAMPTZ,
    failure_reason VARCHAR(255),
    origin VARCHAR(16) NOT NULL DEFAULT 'rust',
    published_at TIMESTAMPTZ,
    UNIQUE (correlation_id)
);

CREATE TABLE IF NOT EXISTS audit_events (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    action VARCHAR(100) NOT NULL,
    entity_type VARCHAR(80) NOT NULL,
    entity_id VARCHAR(80) NOT NULL,
    actor_id UUID REFERENCES users (id),
    metadata_json JSON NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS one_active_tool_placement
    ON tool_placements (tool_id) WHERE removed_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS one_active_compartment_placement
    ON tool_placements (compartment_id) WHERE removed_at IS NULL;

INSERT INTO _sqlx_migrations (version, description, success)
VALUES (20261123000001, 'baseline', TRUE)
ON CONFLICT (version) DO NOTHING;
