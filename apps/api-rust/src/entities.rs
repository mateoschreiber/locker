//! Declarative per-entity specs driving the generic admin CRUD, mirroring
//! Python's `add_admin_routes` but with real payload validation (audit #11).

use crate::error::ApiError;
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub enum Kind {
    Text { max: usize },
    TextOpt { max: usize },
    IntPos,
    UuidRef,
}

pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub editable: bool,
    pub default: Option<&'static str>,
}

pub struct Entity {
    pub path: &'static str,
    pub table: &'static str,
    /// SQLAlchemy class name, used as `entity_type` in audit rows (Python parity).
    pub class_name: &'static str,
    pub action: &'static str,
    pub has_status: bool,
    pub searchable: &'static [&'static str],
    pub fields: &'static [Field],
}

const fn text(name: &'static str, max: usize) -> Field {
    Field {
        name,
        kind: Kind::Text { max },
        required: true,
        editable: true,
        default: None,
    }
}

const fn text_opt(name: &'static str, max: usize) -> Field {
    Field {
        name,
        kind: Kind::TextOpt { max },
        required: false,
        editable: true,
        default: None,
    }
}

const fn uuid_ref(name: &'static str) -> Field {
    Field {
        name,
        kind: Kind::UuidRef,
        required: true,
        editable: true,
        default: None,
    }
}

const fn status(default: &'static str) -> Field {
    Field {
        name: "status",
        kind: Kind::Text { max: 32 },
        required: false,
        editable: true,
        default: Some(default),
    }
}

pub const BRANCHES: Entity = Entity {
    path: "branches",
    table: "branches",
    class_name: "Branch",
    action: "BRANCH",
    has_status: true,
    searchable: &["code", "name"],
    fields: &[text("code", 64), text("name", 160), status("ACTIVE")],
};

pub const ROLES: Entity = Entity {
    path: "roles",
    table: "roles",
    class_name: "Role",
    action: "ROLE",
    has_status: false,
    searchable: &["code", "name"],
    fields: &[text("code", 32), text("name", 100)],
};

pub const USERS: Entity = Entity {
    path: "users",
    table: "users",
    class_name: "User",
    action: "USER",
    has_status: true,
    searchable: &["username", "display_name"],
    fields: &[
        text("username", 80),
        text("display_name", 160),
        Field {
            name: "password",
            kind: Kind::TextOpt { max: 255 },
            required: false,
            editable: true,
            default: Some("admin"),
        },
        status("ACTIVE"),
    ],
};

pub const MEMBERSHIPS: Entity = Entity {
    path: "memberships",
    table: "memberships",
    class_name: "Membership",
    action: "MEMBERSHIP",
    has_status: true,
    searchable: &[],
    fields: &[
        uuid_ref("user_id"),
        uuid_ref("branch_id"),
        uuid_ref("role_id"),
        status("ACTIVE"),
    ],
};

pub const LOCKERS: Entity = Entity {
    path: "lockers",
    table: "lockers",
    class_name: "Locker",
    action: "LOCKER",
    has_status: true,
    searchable: &["code", "name"],
    fields: &[
        uuid_ref("branch_id"),
        text("code", 64),
        text("name", 160),
        status("ONLINE"),
    ],
};

pub const COMPARTMENTS: Entity = Entity {
    path: "compartments",
    table: "compartments",
    class_name: "Compartment",
    action: "COMPARTMENT",
    has_status: true,
    searchable: &["code", "name"],
    fields: &[
        uuid_ref("locker_id"),
        text("code", 64),
        text("name", 160),
        Field {
            name: "position",
            kind: Kind::IntPos,
            required: true,
            editable: true,
            default: None,
        },
        status("ACTIVE"),
    ],
};

pub const LOCKS: Entity = Entity {
    path: "locks",
    table: "locker_locks",
    class_name: "LockerLock",
    action: "LOCK",
    has_status: true,
    searchable: &[],
    fields: &[
        uuid_ref("compartment_id"),
        text_opt("hardware_address", 120),
        status("LOCKED"),
    ],
};

pub const CAMERAS: Entity = Entity {
    path: "cameras",
    table: "cameras",
    class_name: "Camera",
    action: "CAMERA",
    has_status: true,
    searchable: &["name"],
    fields: &[
        uuid_ref("locker_id"),
        text("name", 160),
        status("SIMULATED"),
    ],
};

pub const TOOLS: Entity = Entity {
    path: "tools",
    table: "tools",
    class_name: "Tool",
    action: "TOOL",
    has_status: true,
    searchable: &["asset_code", "name"],
    fields: &[
        text("asset_code", 80),
        text("name", 160),
        text_opt("description", 65535),
        text_opt("rfid_tag", 120),
        status("AVAILABLE"),
    ],
};

/// Admin entities with the full generic CRUD.
pub const ADMIN: &[Entity] = &[
    BRANCHES,
    ROLES,
    USERS,
    MEMBERSHIPS,
    LOCKERS,
    COMPARTMENTS,
    LOCKS,
    CAMERAS,
    TOOLS,
];

/// Read-only list entities served by the same generic list handler.
pub const AUTHORIZATIONS: Entity = Entity {
    path: "authorizations",
    table: "authorizations",
    class_name: "Authorization",
    action: "AUTHORIZATION",
    has_status: true,
    searchable: &[],
    fields: &[],
};

pub const LOANS: Entity = Entity {
    path: "loans",
    table: "loans",
    class_name: "Loan",
    action: "LOAN",
    has_status: true,
    searchable: &[],
    fields: &[],
};

pub const OPERATIONS: Entity = Entity {
    path: "operations",
    table: "locker_operations",
    class_name: "LockerOperation",
    action: "OPERATION",
    has_status: true,
    searchable: &[],
    fields: &[],
};

pub const AUDIT: Entity = Entity {
    path: "audit",
    table: "audit_events",
    class_name: "AuditEvent",
    action: "AUDIT",
    has_status: false,
    searchable: &[],
    fields: &[],
};

/// A validated field value ready to bind.
pub enum Val {
    Text(String),
    Uuid(Uuid),
    Int(i64),
}

/// Extracts and validates one field from a JSON payload.
/// `null` counts as absent; absent + required is a validation error.
pub fn field_value(field: &Field, payload: &Value) -> Result<Option<Val>, ApiError> {
    let Some(raw) = payload.get(field.name) else {
        return Ok(None);
    };
    if raw.is_null() {
        if field.required {
            return Err(ApiError::validation(&format!(
                "Campo requerido: '{}'",
                field.name
            )));
        }
        return Ok(None);
    }
    match field.kind {
        Kind::Text { max } | Kind::TextOpt { max } => {
            let Some(value) = raw.as_str() else {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': se requiere texto",
                    field.name
                )));
            };
            if field.required && value.is_empty() {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': no puede estar vacío",
                    field.name
                )));
            }
            if value.chars().count() > max {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': máximo {} caracteres",
                    field.name, max
                )));
            }
            Ok(Some(Val::Text(value.to_owned())))
        }
        Kind::IntPos => {
            let Some(value) = raw.as_i64() else {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': se requiere un número entero",
                    field.name
                )));
            };
            if value < 1 {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': debe ser mayor o igual a 1",
                    field.name
                )));
            }
            Ok(Some(Val::Int(value)))
        }
        Kind::UuidRef => {
            let Some(value) = raw.as_str() else {
                return Err(ApiError::validation(&format!(
                    "Campo '{}': se requiere un UUID",
                    field.name
                )));
            };
            value.parse::<Uuid>().map(Val::Uuid).map(Some).map_err(|_| {
                ApiError::validation(&format!("Campo '{}': UUID inválido", field.name))
            })
        }
    }
}

/// Parses a required UUID field from a payload (auth, placements, authorizations).
pub fn uuid_field(payload: &Value, name: &str) -> Result<Uuid, ApiError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            ApiError::validation(&format!("Campo '{}': UUID requerido o inválido", name))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_text_field_is_validated() {
        let field = text("code", 4);
        let payload = serde_json::json!({ "code": "abc" });
        assert!(matches!(
            field_value(&field, &payload),
            Ok(Some(Val::Text(_)))
        ));
        let long = serde_json::json!({ "code": "abcde" });
        assert!(field_value(&field, &long).is_err());
        // Absent fields return Ok(None); create() reports the missing-field error.
        assert!(matches!(
            field_value(&field, &serde_json::json!({})),
            Ok(None)
        ));
        assert!(field_value(&field, &serde_json::json!({ "code": 5 })).is_err());
    }

    #[test]
    fn null_counts_as_absent_but_rejected_when_required() {
        let field = uuid_ref("tool_id");
        assert!(field_value(&field, &serde_json::json!({ "tool_id": null })).is_err());
        let optional = text_opt("rfid_tag", 4);
        assert!(matches!(
            field_value(&optional, &serde_json::json!({ "rfid_tag": null })),
            Ok(None)
        ));
    }

    #[test]
    fn position_must_be_positive() {
        let field = Field {
            name: "position",
            kind: Kind::IntPos,
            required: true,
            editable: true,
            default: None,
        };
        assert!(matches!(
            field_value(&field, &serde_json::json!({ "position": 1 })),
            Ok(Some(Val::Int(1)))
        ));
        assert!(field_value(&field, &serde_json::json!({ "position": 0 })).is_err());
        assert!(field_value(&field, &serde_json::json!({ "position": 1.5 })).is_err());
    }

    #[test]
    fn uuid_field_parses_or_rejects() {
        let payload = serde_json::json!({ "tool_id": "00000000-0000-0000-0000-000000000001" });
        assert!(uuid_field(&payload, "tool_id").is_ok());
        assert!(uuid_field(&serde_json::json!({}), "tool_id").is_err());
    }
}
