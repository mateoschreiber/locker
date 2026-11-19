use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row};
use uuid::Uuid;

/// Serializes a database row like Python's `dump()`: UUID → string, timestamps → ISO 8601.
/// Never emits the `password` column so credentials stay out of every response
/// (Python leaked it in every users list; the frontend never reads it).
pub fn dump_row(row: &PgRow) -> Value {
    let mut object = Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        if column.name() == "password" {
            continue;
        }
        object.insert(column.name().to_owned(), column_value(row, index));
    }
    Value::Object(object)
}

/// Probes the column type by decoding: sqlx rejects a type mismatch, so the
/// first successful try_get identifies the column type. NULL decodes as `null`.
fn column_value(row: &PgRow, index: usize) -> Value {
    if let Ok(value) = row.try_get::<Option<Uuid>, usize>(index) {
        return value
            .map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<DateTime<Utc>>, usize>(index) {
        return value
            .map(|v| Value::String(v.to_rfc3339()))
            .unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<Value>, usize>(index) {
        return value.unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<bool>, usize>(index) {
        return value.map(Value::Bool).unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<i16>, usize>(index) {
        return value
            .map(|v| Value::from(i64::from(v)))
            .unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<i32>, usize>(index) {
        return value
            .map(|v| Value::from(i64::from(v)))
            .unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<i64>, usize>(index) {
        return value.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<f64>, usize>(index) {
        return value.map(Value::from).unwrap_or(Value::Null);
    }
    if let Ok(value) = row.try_get::<Option<String>, usize>(index) {
        return value.map(Value::String).unwrap_or(Value::Null);
    }
    Value::Null
}
