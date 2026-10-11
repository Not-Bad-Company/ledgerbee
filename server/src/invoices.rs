use crate::db;
use crate::entities::{self, INVOICES};
use crate::error::{ApiError, ApiResult};
use crate::tax::pct;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RateTotal {
    pub rate: String,
    pub base_cents: i64,
    pub vat_cents: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Totals {
    pub per_rate: Vec<RateTotal>,
    pub excl_cents: i64,
    pub vat_cents: i64,
    pub incl_cents: i64,
}

pub fn line_amount(quantity_milli: i64, unit_cents: i64) -> i64 {
    let n = quantity_milli as i128 * unit_cents as i128;
    let q = (n.abs() + 500) / 1000;
    (if n < 0 { -q } else { q }) as i64
}

fn rate_pct(rate: &str) -> i64 {
    rate.parse().unwrap_or(0)
}

pub fn totals(scheme: &str, lines: &[(i64, i64, String)]) -> Totals {
    let mut per_rate: Vec<RateTotal> = vec![];
    for (q, unit, rate) in lines {
        let rate = if scheme == "nl" { rate.clone() } else { "0".to_string() };
        let amount = line_amount(*q, *unit);
        match per_rate.iter_mut().find(|r| r.rate == rate) {
            Some(r) => r.base_cents += amount,
            None => per_rate.push(RateTotal { rate, base_cents: amount, vat_cents: 0 }),
        }
    }
    per_rate.sort_by_key(|r| -rate_pct(&r.rate));
    for r in &mut per_rate {
        r.vat_cents = pct(r.base_cents, rate_pct(&r.rate));
    }
    let excl_cents = per_rate.iter().map(|r| r.base_cents).sum();
    let vat_cents = per_rate.iter().map(|r| r.vat_cents).sum();
    Totals { per_rate, excl_cents, vat_cents, incl_cents: excl_cents + vat_cents }
}

pub fn totals_of(invoice: &Value) -> Totals {
    let scheme = invoice["vat_scheme"].as_str().unwrap_or("nl");
    let lines: Vec<(i64, i64, String)> = invoice["lines"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|l| {
                    (
                        l["quantity_milli"].as_i64().unwrap_or(0),
                        l["unit_cents"].as_i64().unwrap_or(0),
                        l["vat_rate"].as_str().unwrap_or("21").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    totals(scheme, &lines)
}

pub fn final_between(conn: &Connection, from: &str, to: &str) -> ApiResult<Vec<Value>> {
    let q = entities::ListQuery { from: Some(from.into()), to: Some(to.into()), ..Default::default() };
    Ok(entities::list(conn, &INVOICES, &q)?.into_iter().filter(|i| i["status"] != "concept").collect())
}

pub fn credit(conn: &mut Connection, id: &str) -> ApiResult<Value> {
    let inv = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    if inv["status"] == "concept" {
        return Err(ApiError::bad("Een concept pas je gewoon aan; crediteren kan alleen bij een definitieve factuur"));
    }
    if !inv["credit_of"].as_str().unwrap_or("").is_empty() {
        return Err(ApiError::bad("Dit is zelf al een creditfactuur"));
    }
    let number = inv["number"].as_str().unwrap_or("");
    let lines: Vec<Value> = inv["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            serde_json::json!({
                "description": l["description"],
                "quantity_milli": -l["quantity_milli"].as_i64().unwrap_or(1000),
                "unit_cents": l["unit_cents"],
                "vat_rate": l["vat_rate"],
            })
        })
        .collect();
    let body = serde_json::json!({
        "client_id": inv["client_id"],
        "date": db::today().to_string(),
        "due_date": "",
        "status": "concept",
        "vat_scheme": inv["vat_scheme"],
        "note": format!("Creditering van factuur {number}"),
        "lines": lines,
    });
    let created = entities::create(conn, &INVOICES, &body)?;
    let new_id = created["id"].as_str().unwrap_or_default().to_string();
    conn.execute("UPDATE invoices SET credit_of = ?1 WHERE id = ?2", [id, &new_id])?;
    entities::get(conn, &INVOICES, &new_id)?.ok_or_else(ApiError::not_found)
}

pub fn finalize(conn: &mut Connection, id: &str) -> ApiResult<Value> {
    let inv = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    if inv["status"] != "concept" {
        return Err(ApiError::bad("Deze factuur is al definitief"));
    }
    if inv["client_id"].is_null() {
        return Err(ApiError::bad("Kies eerst een klant"));
    }
    if inv["lines"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        return Err(ApiError::bad("Een factuur zonder regels kan niet definitief"));
    }
    let date = inv["date"].as_str().unwrap_or_default().to_string();
    if date.len() != 10 {
        return Err(ApiError::bad("Ongeldige factuurdatum"));
    }
    let year = &date[..4];
    let tx = conn.transaction()?;
    let last: Option<String> = tx
        .query_row(
            "SELECT number FROM invoices WHERE number LIKE ?1 ORDER BY number DESC LIMIT 1",
            [format!("{year}-%")],
            |r| r.get(0),
        )
        .ok();
    let next = last.and_then(|n| n[5..].parse::<u32>().ok()).unwrap_or(0) + 1;
    let number = format!("{year}-{next:03}");
    let due = if inv["due_date"].as_str().unwrap_or_default().is_empty() {
        let days: i64 = tx
            .query_row("SELECT payment_days FROM clients WHERE id = ?1", [inv["client_id"].as_str()], |r| r.get(0))
            .unwrap_or(14);
        let d = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| ApiError::bad("Ongeldige factuurdatum"))?;
        (d + chrono::Duration::days(days)).to_string()
    } else {
        inv["due_date"].as_str().unwrap_or_default().to_string()
    };
    tx.execute(
        "UPDATE invoices SET number = ?1, status = 'verzonden', due_date = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![number, due, db::now(), id],
    )?;
    let after = entities::get(&tx, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    entities::audit(&tx, "invoices", id, "finalize", Some(&inv), Some(&after))?;
    tx.commit()?;
    Ok(after)
}

pub fn set_paid(conn: &mut Connection, id: &str, date: Option<&str>) -> ApiResult<Value> {
    let inv = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    if inv["status"] == "concept" {
        return Err(ApiError::bad("Een concept kan niet betaald zijn"));
    }
    let status = if date.is_some() { "betaald" } else { "verzonden" };
    conn.execute(
        "UPDATE invoices SET paid_at = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![date, status, db::now(), id],
    )?;
    let after = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    entities::audit(conn, "invoices", id, "paid", Some(&inv), Some(&after))?;
    Ok(after)
}

pub fn check_update(conn: &Connection, id: &str, body: &Value) -> ApiResult<()> {
    let inv = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    if let Some(s) = body.get("status") {
        if s != &inv["status"] {
            return Err(ApiError::bad("De status verandert via definitief maken of betaald melden"));
        }
    }
    if inv["status"] != "concept" {
        let content = |k: &str| k == "lines" || INVOICES.fields.iter().any(|(f, _)| *f == k);
        let changes = |k: &str, v: &Value| content(k) && k != "note" && (k == "lines" || v != &inv[k]);
        if body.as_object().map(|o| o.iter().any(|(k, v)| changes(k, v))).unwrap_or(false) {
            return Err(ApiError::bad("Een definitieve factuur kan niet meer veranderen. Maak een creditfactuur."));
        }
    }
    Ok(())
}

pub fn check_delete(conn: &Connection, id: &str) -> ApiResult<()> {
    let inv = entities::get(conn, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    if !inv["number"].is_null() {
        return Err(ApiError::bad("Een genummerde factuur blijft bestaan. Maak een creditfactuur."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn btw_is_computed_per_rate_not_per_line() {
        let t = totals("nl", &[(1000, 33, "21".into()), (1000, 33, "21".into()), (1000, 33, "21".into()), (2000, 500, "9".into())]);
        assert_eq!(t.per_rate[0], RateTotal { rate: "21".into(), base_cents: 99, vat_cents: 21 });
        assert_eq!(t.per_rate[1], RateTotal { rate: "9".into(), base_cents: 1000, vat_cents: 90 });
        assert_eq!(t.incl_cents, 99 + 21 + 1000 + 90);
        let eu = totals("verlegd_eu", &[(1000, 10000, "21".into())]);
        assert_eq!(eu.vat_cents, 0);
        assert_eq!(line_amount(1500, 333), 500);
    }

    #[test]
    fn numbers_run_per_year_and_final_invoices_are_frozen() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        entities::create(&mut c, &entities::CLIENTS, &json!({"id": "c", "name": "Klant", "payment_days": 30})).unwrap();
        for (id, date) in [("a", "2026-10-01"), ("b", "2026-11-01"), ("x", "2027-01-02")] {
            entities::create(&mut c, &INVOICES, &json!({"id": id, "client_id": "c", "date": date,
                "lines": [{"description": "Website", "quantity_milli": 1000, "unit_cents": 125000, "vat_rate": "21"}]})).unwrap();
        }
        assert_eq!(finalize(&mut c, "a").unwrap()["number"], json!("2026-001"));
        let b = finalize(&mut c, "b").unwrap();
        assert_eq!(b["number"], json!("2026-002"));
        assert_eq!(b["due_date"], json!("2026-12-01"));
        assert_eq!(finalize(&mut c, "x").unwrap()["number"], json!("2027-001"));
        assert!(finalize(&mut c, "a").is_err());
        assert!(check_update(&c, "a", &json!({"lines": []})).is_err());
        assert!(check_update(&c, "a", &json!({"note": "bedankt"})).is_ok());
        assert!(check_update(&c, "a", &json!({"id": "a", "note": "x", "created_at": "2026", "client_id": "c"})).is_ok());
        assert!(check_update(&c, "a", &json!({"date": "2026-01-01"})).is_err());
        assert!(check_delete(&c, "a").is_err());
        assert_eq!(set_paid(&mut c, "a", Some("2026-10-10")).unwrap()["status"], json!("betaald"));
        let cr = credit(&mut c, "a").unwrap();
        assert_eq!(cr["credit_of"], json!("a"));
        assert_eq!(cr["status"], json!("concept"));
        let original = entities::get(&c, &INVOICES, "a").unwrap().unwrap();
        assert_eq!(totals_of(&cr).incl_cents, -totals_of(&original).incl_cents, "a credit cancels the invoice exactly");
        assert!(credit(&mut c, cr["id"].as_str().unwrap()).is_err(), "a concept cannot be credited");
    }
}
