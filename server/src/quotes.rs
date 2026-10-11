use crate::db;
use crate::entities::{self, INVOICES, QUOTES};
use crate::error::{ApiError, ApiResult};
use rusqlite::Connection;
use serde_json::{json, Value};

pub fn number(conn: &mut Connection, id: &str) -> ApiResult<Value> {
    let quote = entities::get(conn, &QUOTES, id)?.ok_or_else(ApiError::not_found)?;
    if quote["number"].as_str().is_some() {
        return Ok(quote);
    }
    if quote["client_id"].is_null() {
        return Err(ApiError::bad("Kies eerst een klant"));
    }
    if quote["lines"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        return Err(ApiError::bad("Een offerte zonder regels kan niet weg"));
    }
    let year = db::today().format("%Y").to_string();
    let tx = conn.transaction()?;
    let last: Option<String> =
        tx.query_row("SELECT number FROM quotes WHERE number LIKE ?1 ORDER BY number DESC LIMIT 1", [format!("O{year}-%")], |r| r.get(0)).ok();
    let next = last.and_then(|n| n[6..].parse::<u32>().ok()).unwrap_or(0) + 1;
    let number = format!("O{year}-{next:03}");
    let valid = if quote["valid_until"].as_str().unwrap_or("").is_empty() {
        (db::today() + chrono::Duration::days(30)).to_string()
    } else {
        quote["valid_until"].as_str().unwrap_or_default().to_string()
    };
    tx.execute(
        "UPDATE quotes SET number = ?1, valid_until = ?2, status = CASE status WHEN 'concept' THEN 'verzonden' ELSE status END, updated_at = ?3 WHERE id = ?4",
        [&number, &valid, &db::now(), id],
    )?;
    tx.commit()?;
    entities::audit(conn, "quotes", id, "numbered", None, Some(&json!({ "number": number })))?;
    entities::get(conn, &QUOTES, id)?.ok_or_else(ApiError::not_found)
}

pub fn to_invoice(conn: &mut Connection, id: &str) -> ApiResult<Value> {
    let quote = entities::get(conn, &QUOTES, id)?.ok_or_else(ApiError::not_found)?;
    if let Some(existing) = quote["invoice_id"].as_str().filter(|s| !s.is_empty()) {
        if let Some(inv) = entities::get(conn, &INVOICES, existing)? {
            return Ok(inv);
        }
    }
    let lines: Vec<Value> = quote["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| json!({"description": l["description"], "quantity_milli": l["quantity_milli"], "unit_cents": l["unit_cents"], "vat_rate": l["vat_rate"]}))
        .collect();
    let note = match quote["number"].as_str() {
        Some(n) => format!("Volgens offerte {n}"),
        None => String::new(),
    };
    let inv = entities::create(
        conn,
        &INVOICES,
        &json!({
            "client_id": quote["client_id"],
            "date": db::today().to_string(),
            "due_date": "",
            "status": "concept",
            "vat_scheme": quote["vat_scheme"],
            "note": note,
            "lines": lines,
        }),
    )?;
    let inv_id = inv["id"].as_str().unwrap_or_default().to_string();
    conn.execute("UPDATE quotes SET status = 'geaccepteerd', invoice_id = ?1, updated_at = ?2 WHERE id = ?3", [&inv_id, &db::now(), id])?;
    entities::audit(conn, "quotes", id, "invoiced", None, Some(&json!({ "invoice": inv_id })))?;
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoices::totals_of;

    #[test]
    fn a_quote_is_numbered_once_and_becomes_one_invoice() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        entities::create(&mut c, &entities::CLIENTS, &json!({"id": "c", "name": "Klant"})).unwrap();
        entities::create(&mut c, &QUOTES, &json!({"id": "q", "client_id": "c", "date": "2026-10-01",
            "lines": [{"description": "Website", "quantity_milli": 1000, "unit_cents": 150000, "vat_rate": "21"}]})).unwrap();
        let q = number(&mut c, "q").unwrap();
        let n = q["number"].as_str().unwrap().to_string();
        assert!(n.starts_with('O') && n.ends_with("-001"));
        assert_eq!(q["status"], json!("verzonden"));
        assert!(!q["valid_until"].as_str().unwrap().is_empty());
        assert_eq!(number(&mut c, "q").unwrap()["number"], json!(n), "numbered once");
        let inv = to_invoice(&mut c, "q").unwrap();
        assert_eq!(inv["status"], json!("concept"));
        assert_eq!(totals_of(&inv).incl_cents, 181500);
        assert_eq!(to_invoice(&mut c, "q").unwrap()["id"], inv["id"], "converting twice gives the same invoice");
        assert_eq!(entities::get(&c, &QUOTES, "q").unwrap().unwrap()["status"], json!("geaccepteerd"));
    }
}
