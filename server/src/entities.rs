//! Generic create/read/update/delete for the bookkeeping tables.
//!
//! Each table is described once (`Entity`): its writable fields and their
//! kinds, the date that decides which period a row belongs to, and an
//! optional child table of lines that travels inside the parent as `lines`.
//! Every write is checked against filed periods and written to `audit_log`.
//!
//! Creates are idempotent on `id`: the app makes the id itself, so a create
//! replayed from its offline outbox returns the row that already exists.

use crate::db;
use crate::error::{ApiError, ApiResult};
use rusqlite::types::ValueRef;
use rusqlite::{params_from_iter, Connection, ToSql};
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Text,
    /// Text that may be null (foreign keys, optional dates).
    OptText,
    Int,
    Bool,
}

pub struct Lines {
    pub table: &'static str,
    pub parent_key: &'static str,
    pub fields: &'static [(&'static str, Kind)],
}

pub struct Entity {
    pub name: &'static str,
    pub table: &'static str,
    pub fields: &'static [(&'static str, Kind)],
    /// The date that places a row in a btw quarter / tax year.
    pub date_field: Option<&'static str>,
    /// Whether a filed btw quarter locks it (costs and sales), or only a filed
    /// tax year (hours, trips, assets).
    pub btw_relevant: bool,
    pub order_by: &'static str,
    pub lines: Option<Lines>,
}

use Kind::*;

pub const DOCUMENTS: Entity = Entity {
    name: "documents",
    table: "documents",
    fields: &[
        ("kind", Text),
        ("date", Text),
        ("supplier_name", Text),
        ("supplier_id", OptText),
        ("total_cents", Int),
        ("file_sha", Text),
        ("file_type", Text),
        ("ocr_text", Text),
        ("status", Text),
        ("note", Text),
        ("paid_private", Bool),
        ("parse_notes", Text),
    ],
    date_field: Some("date"),
    btw_relevant: true,
    order_by: "date DESC, created_at DESC",
    lines: Some(Lines {
        table: "document_lines",
        parent_key: "document_id",
        fields: &[
            ("description", Text),
            ("amount_excl_cents", Int),
            ("vat_rate", Text),
            ("vat_cents", Int),
            ("category_id", OptText),
            ("business_pct", Int),
        ],
    }),
};

pub const INVOICES: Entity = Entity {
    name: "invoices",
    table: "invoices",
    // `number`, `sent_at` and `paid_at` are set by their own endpoints only.
    fields: &[("client_id", OptText), ("date", Text), ("due_date", Text), ("status", Text), ("vat_scheme", Text), ("note", Text)],
    date_field: Some("date"),
    btw_relevant: true,
    order_by: "date DESC, created_at DESC",
    lines: Some(Lines {
        table: "invoice_lines",
        parent_key: "invoice_id",
        fields: &[("description", Text), ("quantity_milli", Int), ("unit_cents", Int), ("vat_rate", Text)],
    }),
};

pub const CLIENTS: Entity = Entity {
    name: "clients",
    table: "clients",
    fields: &[
        ("name", Text),
        ("contact", Text),
        ("email", Text),
        ("street", Text),
        ("postcode", Text),
        ("city", Text),
        ("country", Text),
        ("kvk", Text),
        ("vat_id", Text),
        ("is_business", Bool),
        ("payment_days", Int),
    ],
    date_field: None,
    btw_relevant: false,
    order_by: "name COLLATE NOCASE",
    lines: None,
};

pub const SUPPLIERS: Entity = Entity {
    name: "suppliers",
    table: "suppliers",
    fields: &[
        ("name", Text),
        ("kvk", Text),
        ("vat_id", Text),
        ("default_category_id", OptText),
        ("default_vat_rate", Text),
        ("match_terms", Text),
    ],
    date_field: None,
    btw_relevant: false,
    order_by: "name COLLATE NOCASE",
    lines: None,
};

pub const CATEGORIES: Entity = Entity {
    name: "categories",
    table: "categories",
    fields: &[("name", Text), ("ib_group", Text), ("deductible_pct", Int), ("vat_deductible", Bool), ("archived", Bool)],
    date_field: None,
    btw_relevant: false,
    order_by: "archived, name COLLATE NOCASE",
    lines: None,
};

pub const BANK: Entity = Entity {
    name: "bank_transactions",
    table: "bank_transactions",
    // Rows come from an import; only the match and the private flag change.
    fields: &[("matched_kind", Text), ("matched_id", Text), ("ignored", Bool)],
    date_field: Some("date"),
    btw_relevant: false,
    order_by: "date DESC, created_at DESC",
    lines: None,
};

pub const ASSETS: Entity = Entity {
    name: "assets",
    table: "assets",
    fields: &[
        ("name", Text),
        ("purchase_date", Text),
        ("cost_cents", Int),
        ("useful_years", Int),
        ("residual_cents", Int),
        ("business_pct", Int),
        ("document_id", OptText),
        ("disposed_date", OptText),
    ],
    date_field: Some("purchase_date"),
    btw_relevant: false,
    order_by: "purchase_date DESC",
    lines: None,
};

pub const HOURS: Entity = Entity {
    name: "hours",
    table: "hours",
    fields: &[("date", Text), ("minutes", Int), ("client_id", OptText), ("description", Text)],
    date_field: Some("date"),
    btw_relevant: false,
    order_by: "date DESC, created_at DESC",
    lines: None,
};

pub const TRIPS: Entity = Entity {
    name: "trips",
    table: "trips",
    fields: &[
        ("date", Text),
        ("origin", Text),
        ("destination", Text),
        ("km_tenths", Int),
        ("return_trip", Bool),
        ("purpose", Text),
        ("client_id", OptText),
    ],
    date_field: Some("date"),
    btw_relevant: false,
    order_by: "date DESC, created_at DESC",
    lines: None,
};

pub const ALL: &[&Entity] = &[&DOCUMENTS, &INVOICES, &CLIENTS, &SUPPLIERS, &CATEGORIES, &BANK, &ASSETS, &HOURS, &TRIPS];

pub fn by_name(name: &str) -> Option<&'static Entity> {
    ALL.iter().copied().find(|e| e.name == name)
}

fn kind_of(fields: &[(&str, Kind)], name: &str) -> Option<Kind> {
    fields.iter().find(|(f, _)| *f == name).map(|(_, k)| *k)
}

/// A JSON value as a parameter for a column of the given kind.
fn to_param(kind: Kind, field: &str, v: &Value) -> ApiResult<Box<dyn ToSql>> {
    let bad = || ApiError::bad(format!("Ongeldige waarde voor {field}"));
    Ok(match (kind, v) {
        (OptText, Value::Null) => Box::new(Option::<String>::None),
        (Text | OptText, Value::String(s)) => Box::new(s.trim().to_string()),
        (Int, Value::Number(n)) => Box::new(n.as_i64().ok_or_else(bad)?),
        (Bool, Value::Bool(b)) => Box::new(*b as i64),
        (Bool, Value::Number(n)) => Box::new((n.as_i64().ok_or_else(bad)? != 0) as i64),
        _ => return Err(bad()),
    })
}

/// A row as a JSON object; integer columns named in `bools` become booleans.
pub fn row_json(row: &rusqlite::Row, bools: &[&str]) -> rusqlite::Result<Map<String, Value>> {
    let mut out = Map::new();
    let stmt = row.as_ref();
    for i in 0..stmt.column_count() {
        let name = stmt.column_name(i)?.to_string();
        let value = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) if bools.contains(&name.as_str()) => Value::Bool(n != 0),
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(f) => Value::from(f),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(_) => Value::Null,
        };
        out.insert(name, value);
    }
    Ok(out)
}

fn bools(fields: &'static [(&'static str, Kind)]) -> Vec<&'static str> {
    fields.iter().filter(|(_, k)| *k == Bool).map(|(f, _)| *f).collect()
}

pub fn query_json(conn: &Connection, sql: &str, params: &[&dyn ToSql], bools: &[&str]) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |r| row_json(r, bools).map(Value::Object))?;
    rows.collect()
}

pub fn get(conn: &Connection, e: &Entity, id: &str) -> ApiResult<Option<Value>> {
    let sql = format!("SELECT * FROM {} WHERE id = ?1", e.table);
    let mut found = query_json(conn, &sql, &[&id], &bools(e.fields))?;
    let Some(mut row) = found.pop() else { return Ok(None) };
    if let Some(lines) = &e.lines {
        let sql = format!("SELECT * FROM {} WHERE {} = ?1 ORDER BY position", lines.table, lines.parent_key);
        let items = query_json(conn, &sql, &[&id], &bools(lines.fields))?;
        row["lines"] = Value::Array(items);
    }
    Ok(Some(row))
}

#[derive(Default, Debug, serde::Deserialize)]
pub struct ListQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

pub fn list(conn: &Connection, e: &Entity, q: &ListQuery) -> ApiResult<Vec<Value>> {
    let mut sql = format!("SELECT id FROM {} WHERE 1=1", e.table);
    let mut params: Vec<String> = vec![];
    if let Some(field) = e.date_field {
        if let Some(from) = &q.from {
            params.push(from.clone());
            sql += &format!(" AND {field} >= ?{}", params.len());
        }
        if let Some(to) = &q.to {
            params.push(to.clone());
            sql += &format!(" AND {field} <= ?{}", params.len());
        }
    }
    if let (Some(status), true) = (&q.status, kind_of(e.fields, "status").is_some()) {
        params.push(status.clone());
        sql += &format!(" AND status = ?{}", params.len());
    }
    sql += &format!(" ORDER BY {} LIMIT {}", e.order_by, q.limit.unwrap_or(5000).clamp(1, 5000));
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params.iter()), |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(row) = get(conn, e, &id)? {
            out.push(row);
        }
    }
    Ok(out)
}

/// The filed period that locks `date` for this entity, if any.
pub fn lock_for(conn: &Connection, e: &Entity, date: &str) -> ApiResult<Option<String>> {
    if date.len() < 7 {
        return Ok(None);
    }
    let year = &date[..4];
    let month: u32 = date[5..7].parse().unwrap_or(0);
    let quarter = format!("{year}-Q{}", (month.max(1) - 1) / 3 + 1);
    let filed = |kind: &str, key: &str| -> rusqlite::Result<bool> {
        conn.query_row("SELECT status = 'ingediend' FROM periods WHERE kind = ?1 AND key = ?2", [kind, key], |r| r.get(0))
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(false) } else { Err(e) })
    };
    if filed("ib_year", year)? {
        return Ok(Some(format!("De aangifte inkomstenbelasting {year} is ingediend")));
    }
    if e.btw_relevant && filed("btw_q", &quarter)? {
        return Ok(Some(format!("De btw-aangifte {quarter} is ingediend; corrigeer dit in het volgende kwartaal")));
    }
    Ok(None)
}

fn check_lock(conn: &Connection, e: &Entity, row: Option<&Value>) -> ApiResult<()> {
    if let (Some(field), Some(row)) = (e.date_field, row) {
        if let Some(date) = row.get(field).and_then(Value::as_str) {
            if let Some(why) = lock_for(conn, e, date)? {
                return Err(ApiError::locked(why));
            }
        }
    }
    Ok(())
}

pub fn audit(conn: &Connection, table: &str, id: &str, action: &str, before: Option<&Value>, after: Option<&Value>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (at, tbl, row_id, action, before, after) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![db::now(), table, id, action, before.map(Value::to_string), after.map(Value::to_string)],
    )?;
    Ok(())
}

fn write_lines(conn: &Connection, lines: &Lines, parent: &str, items: &[Value]) -> ApiResult<()> {
    conn.execute(&format!("DELETE FROM {} WHERE {} = ?1", lines.table, lines.parent_key), [parent])?;
    let now = db::now();
    for (pos, item) in items.iter().enumerate() {
        let obj = item.as_object().ok_or_else(|| ApiError::bad("Een regel is geen object"))?;
        let mut cols = vec!["id", lines.parent_key, "position", "created_at", "updated_at"];
        let id = obj.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(db::new_id);
        let mut params: Vec<Box<dyn ToSql>> =
            vec![Box::new(id), Box::new(parent.to_string()), Box::new(pos as i64), Box::new(now.clone()), Box::new(now.clone())];
        for (field, kind) in lines.fields {
            if let Some(v) = obj.get(*field) {
                cols.push(field);
                params.push(to_param(*kind, field, v)?);
            }
        }
        let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
        let sql = format!("INSERT INTO {} ({}) VALUES ({})", lines.table, cols.join(", "), marks.join(", "));
        conn.execute(&sql, params_from_iter(params.iter().map(|p| p.as_ref())))?;
    }
    Ok(())
}

/// Creates a row from `body`, or returns the existing one with the same id.
pub fn create(conn: &mut Connection, e: &Entity, body: &Value) -> ApiResult<Value> {
    let obj = body.as_object().ok_or_else(|| ApiError::bad("Verwacht een object"))?;
    let id = obj.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(db::new_id);
    if let Some(existing) = get(conn, e, &id)? {
        return Ok(existing);
    }
    check_lock(conn, e, Some(body))?;
    let tx = conn.transaction()?;
    let now = db::now();
    let mut cols = vec!["id", "created_at", "updated_at"];
    let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(id.clone()), Box::new(now.clone()), Box::new(now)];
    for (field, kind) in e.fields {
        if let Some(v) = obj.get(*field) {
            cols.push(field);
            params.push(to_param(*kind, field, v)?);
        }
    }
    let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let sql = format!("INSERT INTO {} ({}) VALUES ({})", e.table, cols.join(", "), marks.join(", "));
    tx.execute(&sql, params_from_iter(params.iter().map(|p| p.as_ref()))).map_err(constraint)?;
    if let (Some(lines), Some(items)) = (&e.lines, obj.get("lines").and_then(Value::as_array)) {
        write_lines(&tx, lines, &id, items)?;
    }
    let after = get(&tx, e, &id)?.ok_or_else(ApiError::not_found)?;
    audit(&tx, e.table, &id, "create", None, Some(&after))?;
    tx.commit()?;
    Ok(after)
}

/// Applies the fields present in `body` (and replaces `lines` when given).
pub fn update(conn: &mut Connection, e: &Entity, id: &str, body: &Value) -> ApiResult<Value> {
    let obj = body.as_object().ok_or_else(|| ApiError::bad("Verwacht een object"))?;
    let before = get(conn, e, id)?.ok_or_else(ApiError::not_found)?;
    check_lock(conn, e, Some(&before))?;
    check_lock(conn, e, Some(body))?;
    let tx = conn.transaction()?;
    let mut sets = vec!["updated_at = ?1".to_string()];
    let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(db::now())];
    for (field, kind) in e.fields {
        if let Some(v) = obj.get(*field) {
            params.push(to_param(*kind, field, v)?);
            sets.push(format!("{field} = ?{}", params.len()));
        }
    }
    params.push(Box::new(id.to_string()));
    let sql = format!("UPDATE {} SET {} WHERE id = ?{}", e.table, sets.join(", "), params.len());
    tx.execute(&sql, params_from_iter(params.iter().map(|p| p.as_ref()))).map_err(constraint)?;
    if let (Some(lines), Some(items)) = (&e.lines, obj.get("lines").and_then(Value::as_array)) {
        write_lines(&tx, lines, id, items)?;
    }
    let after = get(&tx, e, id)?.ok_or_else(ApiError::not_found)?;
    audit(&tx, e.table, id, "update", Some(&before), Some(&after))?;
    tx.commit()?;
    Ok(after)
}

pub fn delete(conn: &mut Connection, e: &Entity, id: &str) -> ApiResult<()> {
    let before = get(conn, e, id)?.ok_or_else(ApiError::not_found)?;
    check_lock(conn, e, Some(&before))?;
    let tx = conn.transaction()?;
    tx.execute(&format!("DELETE FROM {} WHERE id = ?1", e.table), [id]).map_err(constraint)?;
    audit(&tx, e.table, id, "delete", Some(&before), None)?;
    tx.commit()?;
    Ok(())
}

fn constraint(e: rusqlite::Error) -> ApiError {
    match &e {
        rusqlite::Error::SqliteFailure(f, msg) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
            ApiError::bad(format!("Kan niet opslaan: {}", msg.clone().unwrap_or_default()))
        }
        _ => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_is_idempotent_and_lines_travel_with_the_parent() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        let body = json!({"id": "d1", "date": "2026-10-03", "supplier_name": "Hema", "total_cents": 1210, "paid_private": true,
            "lines": [{"description": "Pennen", "amount_excl_cents": 1000, "vat_rate": "21", "vat_cents": 210}]});
        let a = create(&mut c, &DOCUMENTS, &body).unwrap();
        assert_eq!(a["paid_private"], json!(true));
        assert_eq!(a["lines"][0]["vat_cents"], json!(210));
        let b = create(&mut c, &DOCUMENTS, &json!({"id": "d1", "date": "2026-10-04"})).unwrap();
        assert_eq!(b["date"], json!("2026-10-03"), "a replayed create returns the existing row");
        let n: i64 = c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let u = update(&mut c, &DOCUMENTS, "d1", &json!({"lines": []})).unwrap();
        assert_eq!(u["lines"], json!([]));
    }

    #[test]
    fn a_filed_quarter_locks_its_costs_but_not_its_hours() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        create(&mut c, &DOCUMENTS, &json!({"id": "d1", "date": "2026-11-03"})).unwrap();
        c.execute("INSERT INTO periods (kind, key, status) VALUES ('btw_q', '2026-Q4', 'ingediend')", []).unwrap();
        let err = update(&mut c, &DOCUMENTS, "d1", &json!({"note": "x"})).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::CONFLICT);
        assert!(create(&mut c, &DOCUMENTS, &json!({"date": "2026-12-31"})).is_err());
        assert!(create(&mut c, &DOCUMENTS, &json!({"date": "2027-01-01"})).is_ok());
        assert!(create(&mut c, &HOURS, &json!({"date": "2026-11-03", "minutes": 60})).is_ok());
        c.execute("INSERT INTO periods (kind, key, status) VALUES ('ib_year', '2026', 'ingediend')", []).unwrap();
        assert!(create(&mut c, &HOURS, &json!({"date": "2026-11-04", "minutes": 60})).is_err());
    }

    #[test]
    fn wrong_types_are_refused() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        assert!(create(&mut c, &HOURS, &json!({"date": "2026-11-03", "minutes": "veel"})).is_err());
    }
}
