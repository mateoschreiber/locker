-- shortcut: mirrors the Alembic migrations 20260817_0002/0003 for isolated Rust
-- tests; keep in sync if a migration changes, drop it when Python retires (Phase 4).
DROP TABLE IF EXISTS audit_events, locker_operations, loans, authorizations, tool_placements,
    tools, cameras, locker_locks, compartments, lockers, memberships, users, roles, branches CASCADE;

CREATE TABLE branches (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    code VARCHAR(64) NOT NULL,
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (code)
);
CREATE TABLE roles (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    code VARCHAR(32) NOT NULL,
    name VARCHAR(100) NOT NULL,
    UNIQUE (code)
);
CREATE TABLE users (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    username VARCHAR(80) NOT NULL,
    display_name VARCHAR(160) NOT NULL,
    password VARCHAR(255) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (username)
);
CREATE TABLE memberships (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_id UUID NOT NULL REFERENCES users (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    role_id UUID NOT NULL REFERENCES roles (id),
    status VARCHAR(32) NOT NULL,
    UNIQUE (user_id, branch_id, role_id)
);
CREATE TABLE lockers (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    branch_id UUID NOT NULL REFERENCES branches (id),
    code VARCHAR(64) NOT NULL,
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL,
    UNIQUE (branch_id, code)
);
CREATE TABLE compartments (
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
CREATE TABLE locker_locks (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    compartment_id UUID NOT NULL REFERENCES compartments (id),
    hardware_address VARCHAR(120),
    status VARCHAR(32) NOT NULL,
    UNIQUE (compartment_id)
);
CREATE TABLE cameras (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locker_id UUID NOT NULL REFERENCES lockers (id),
    name VARCHAR(160) NOT NULL,
    status VARCHAR(32) NOT NULL
);
CREATE TABLE tools (
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
CREATE TABLE tool_placements (
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
CREATE TABLE authorizations (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    tool_id UUID NOT NULL REFERENCES tools (id),
    user_id UUID NOT NULL REFERENCES users (id),
    branch_id UUID NOT NULL REFERENCES branches (id),
    status VARCHAR(32) NOT NULL,
    approved_at TIMESTAMPTZ
);
CREATE TABLE loans (
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
CREATE TABLE locker_operations (
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
    origin VARCHAR(16) NOT NULL DEFAULT 'python',
    published_at TIMESTAMPTZ,
    UNIQUE (correlation_id)
);
CREATE TABLE audit_events (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    action VARCHAR(100) NOT NULL,
    entity_type VARCHAR(80) NOT NULL,
    entity_id VARCHAR(80) NOT NULL,
    actor_id UUID REFERENCES users (id),
    metadata_json JSON NOT NULL
);

CREATE UNIQUE INDEX one_active_tool_placement ON tool_placements (tool_id) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX one_active_compartment_placement ON tool_placements (compartment_id) WHERE removed_at IS NULL;
