//! The quarterly btw-aangifte, in the Belastingdienst's rubrieken.
//!
//! Factuurstelsel: sales count by invoice date, costs by document date.
//! Only confirmed documents and final invoices count; what is still a
//! concept is listed as a warning so nothing is forgotten silently.

use crate::entities::{self, DOCUMENTS};
use crate::error::{ApiError, ApiResult};
use crate::invoices;
use crate::tax::pct;
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Debug, Clone, Default)]
pub struct Rubriek {
    pub code: &'static str,
    pub label: &'static str,
    pub base_cents: i64,
    pub vat_cents: i64,
    /// What goes in the form: whole euros, rounded in your favour (omzet and
    /// verschuldigde btw down, voorbelasting up).
    pub base_eur: i64,
    pub vat_eur: i64,
    pub has_base: bool,
    /// The invoices and costs behind the figure, so it can be checked before filing.
    pub sources: Vec<Source>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Source {
    /// `invoice` or `document`.
    pub kind: &'static str,
    pub id: String,
    pub date: String,
    pub name: String,
    pub base_cents: i64,
    pub vat_cents: i64,
}

/// Adds one record's share to a rubriek's list, merging lines of the same record.
fn note(sources: &mut HashMap<&'static str, Vec<Source>>, code: &'static str, src: Source) {
    let list = sources.entry(code).or_default();
    match list.iter_mut().find(|x| x.id == src.id && x.kind == src.kind) {
        Some(x) => {
            x.base_cents += src.base_cents;
            x.vat_cents += src.vat_cents;
        }
        None => list.push(src),
    }
}

#[derive(Serialize, Debug)]
pub struct BtwReport {
    pub key: String,
    pub from: String,
    pub to: String,
    pub deadline: String,
    pub rubrieken: Vec<Rubriek>,
    /// 5g in cents (positive: to pay) and in the form's whole euros.
    pub total_cents: i64,
    pub total_eur: i64,
    pub warnings: Vec<String>,
    pub status: String,
    pub filed_at: Option<String>,
}

/// `2026-Q4` → (first day, last day, deadline: last day of the next month).
pub fn quarter_bounds(key: &str) -> ApiResult<(NaiveDate, NaiveDate, NaiveDate)> {
    let bad = || ApiError::bad("Kwartaal als 2026-Q4");
    let (y, q) = key.split_once("-Q").ok_or_else(bad)?;
    let (y, q): (i32, u32) = (y.parse().map_err(|_| bad())?, q.parse().map_err(|_| bad())?);
    if !(1..=4).contains(&q) {
        return Err(bad());
    }
    let from = NaiveDate::from_ymd_opt(y, (q - 1) * 3 + 1, 1).ok_or_else(bad)?;
    let next = if q == 4 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, q * 3 + 1, 1) }.ok_or_else(bad)?;
    let to = next.pred_opt().ok_or_else(bad)?;
    let after_next = if next.month() == 12 { NaiveDate::from_ymd_opt(next.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(next.year(), next.month() + 1, 1) }
        .ok_or_else(bad)?;
    Ok((from, to, after_next.pred_opt().ok_or_else(bad)?))
}

pub fn quarter_of(d: NaiveDate) -> String {
    format!("{}-Q{}", d.year(), (d.month() - 1) / 3 + 1)
}

fn floor_eur(c: i64) -> i64 {
    c.div_euclid(100)
}
fn ceil_eur(c: i64) -> i64 {
    -(-c).div_euclid(100)
}

struct Cat {
    vat_deductible: bool,
}

fn categories(conn: &Connection) -> ApiResult<HashMap<String, Cat>> {
    let mut stmt = conn.prepare("SELECT id, vat_deductible FROM categories")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, Cat { vat_deductible: r.get::<_, i64>(1)? != 0 })))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn report(conn: &Connection, key: &str) -> ApiResult<BtwReport> {
    let (from, to, deadline) = quarter_bounds(key)?;
    let (from_s, to_s) = (from.to_string(), to.to_string());
    let mut r: HashMap<&str, (i64, i64)> = HashMap::new();
    let mut add = |code: &'static str, base: i64, vat: i64| {
        let e = r.entry(code).or_default();
        e.0 += base;
        e.1 += vat;
    };
    let mut warnings = vec![];
    let mut sources: HashMap<&'static str, Vec<Source>> = HashMap::new();

    // Sales.
    let client_names: HashMap<String, String> = {
        let mut stmt = conn.prepare("SELECT id, name FROM clients")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for inv in invoices::final_between(conn, &from_s, &to_s)? {
        let t = invoices::totals_of(&inv);
        let scheme = inv["vat_scheme"].as_str().unwrap_or("nl");
        let client = inv["client_id"].as_str().and_then(|c| client_names.get(c)).cloned().unwrap_or_default();
        let name = format!("{} {}", inv["number"].as_str().unwrap_or(""), client).trim().to_string();
        for rt in t.per_rate {
            let (code, vat) = match (scheme, rt.rate.as_str()) {
                ("nl", "21") => ("1a", rt.vat_cents),
                ("nl", "9") => ("1b", rt.vat_cents),
                ("verlegd_eu", _) => ("3b", 0),
                // Services to a business outside the EU are not taxed here.
                _ => ("1e", 0),
            };
            add(code, rt.base_cents, vat);
            let src = Source {
                kind: "invoice",
                id: inv["id"].as_str().unwrap_or("").into(),
                date: inv["date"].as_str().unwrap_or("").into(),
                name: name.clone(),
                base_cents: rt.base_cents,
                vat_cents: vat,
            };
            note(&mut sources, code, src);
        }
    }
    let concepts: i64 = conn.query_row(
        "SELECT count(*) FROM invoices WHERE status = 'concept' AND date BETWEEN ?1 AND ?2",
        [&from_s, &to_s],
        |row| row.get(0),
    )?;
    if concepts > 0 {
        warnings.push(if concepts == 1 { "1 conceptfactuur in dit kwartaal telt nog niet mee".to_string() } else { format!("{concepts} conceptfacturen in dit kwartaal tellen nog niet mee") });
    }

    // Costs.
    let cats = categories(conn)?;
    let q = entities::ListQuery { from: Some(from_s.clone()), to: Some(to_s.clone()), ..Default::default() };
    let mut voorbelasting = 0i64;
    let mut unconfirmed = 0;
    for doc in entities::list(conn, &DOCUMENTS, &q)? {
        if doc["status"] != "bevestigd" {
            unconfirmed += 1;
            continue;
        }
        let src = |base: i64, vat: i64| Source {
            kind: "document",
            id: doc["id"].as_str().unwrap_or("").into(),
            date: doc["date"].as_str().unwrap_or("").into(),
            name: doc["supplier_name"].as_str().unwrap_or("").into(),
            base_cents: base,
            vat_cents: vat,
        };
        for line in doc["lines"].as_array().into_iter().flatten() {
            let base = line["amount_excl_cents"].as_i64().unwrap_or(0);
            let vat = line["vat_cents"].as_i64().unwrap_or(0);
            let business = line["business_pct"].as_i64().unwrap_or(100);
            let deductible =
                line["category_id"].as_str().and_then(|c| cats.get(c)).map(|c| c.vat_deductible).unwrap_or(true);
            let share = |v: i64| if deductible { pct(v, business) } else { 0 };
            match line["vat_rate"].as_str().unwrap_or("") {
                "21" | "9" => {
                    voorbelasting += share(vat);
                    note(&mut sources, "5b", src(base, share(vat)));
                }
                rate @ ("verlegd_eu" | "verlegd_buiten_eu") => {
                    let charged = pct(base, 21);
                    let code = if rate == "verlegd_eu" { "4b" } else { "4a" };
                    add(code, base, charged);
                    note(&mut sources, code, src(base, charged));
                    voorbelasting += share(charged);
                    note(&mut sources, "5b", src(base, share(charged)));
                }
                _ => {}
            }
        }
    }
    if unconfirmed > 0 {
        warnings.push(if unconfirmed == 1 { "1 kostenpost in dit kwartaal is nog niet bevestigd en telt niet mee".to_string() } else { format!("{unconfirmed} kostenposten in dit kwartaal zijn nog niet bevestigd en tellen niet mee") });
    }

    // Business payments from the bank in this quarter with no bon or invoice
    // linked: their btw is missing from 5b until one is found.
    let unlinked: i64 = conn.query_row(
        "SELECT count(*) FROM bank_transactions WHERE amount_cents < 0 AND matched_id = '' AND ignored = 0 AND date BETWEEN ?1 AND ?2",
        [&from_s, &to_s],
        |row| row.get(0),
    )?;
    if unlinked > 0 {
        warnings.push(if unlinked == 1 {
            "1 betaling van de bank in dit kwartaal heeft nog geen bon; zonder bon telt de btw niet mee".to_string()
        } else {
            format!("{unlinked} betalingen van de bank in dit kwartaal hebben nog geen bon; zonder bon telt de btw niet mee")
        });
    }

    let rub = |code: &'static str, label: &'static str, has_base: bool| {
        let (base, vat) = r.get(code).copied().unwrap_or_default();
        let sources = sources.get(code).cloned().unwrap_or_default();
        Rubriek { code, label, base_cents: base, vat_cents: vat, base_eur: floor_eur(base), vat_eur: floor_eur(vat), has_base, sources }
    };
    let mut rubrieken = vec![
        rub("1a", "Leveringen/diensten belast met hoog tarief", true),
        rub("1b", "Leveringen/diensten belast met laag tarief", true),
        rub("1c", "Leveringen/diensten belast met overige tarieven, behalve 0%", true),
        rub("1d", "Privégebruik", true),
        rub("1e", "Leveringen/diensten belast met 0% of niet bij u belast", true),
        rub("2a", "Leveringen/diensten waarbij de btw naar u is verlegd", true),
        rub("3a", "Leveringen naar landen buiten de EU (uitvoer)", true),
        rub("3b", "Leveringen naar of diensten in landen binnen de EU", true),
        rub("3c", "Installatie/afstandsverkopen binnen de EU", true),
        rub("4a", "Leveringen/diensten uit landen buiten de EU", true),
        rub("4b", "Leveringen/diensten uit landen binnen de EU", true),
    ];
    let verschuldigd_cents: i64 = rubrieken.iter().map(|x| x.vat_cents).sum();
    let verschuldigd_eur: i64 = rubrieken.iter().map(|x| x.vat_eur).sum();
    let voor_eur = ceil_eur(voorbelasting);
    rubrieken.push(Rubriek { code: "5a", label: "Verschuldigde btw (rubrieken 1 t/m 4)", vat_cents: verschuldigd_cents, vat_eur: verschuldigd_eur, ..Default::default() });
    let voor_sources = sources.remove("5b").unwrap_or_default();
    rubrieken.push(Rubriek { code: "5b", label: "Voorbelasting", vat_cents: voorbelasting, vat_eur: voor_eur, sources: voor_sources, ..Default::default() });
    let total_cents = verschuldigd_cents - voorbelasting;
    let total_eur = verschuldigd_eur - voor_eur;
    rubrieken.push(Rubriek { code: "5g", label: "Totaal te betalen / terug te vragen", vat_cents: total_cents, vat_eur: total_eur, ..Default::default() });

    let (status, filed_at) = conn
        .query_row("SELECT status, filed_at FROM periods WHERE kind = 'btw_q' AND key = ?1", [key], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap_or_else(|_| ("open".to_string(), None));
    Ok(BtwReport {
        key: key.to_string(),
        from: from_s,
        to: to_s,
        deadline: deadline.to_string(),
        rubrieken,
        total_cents,
        total_eur,
        warnings,
        status,
        filed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::entities::{create, CLIENTS, INVOICES};
    use serde_json::json;

    #[test]
    fn quarter_bounds_and_deadlines() {
        let (f, t, d) = quarter_bounds("2026-Q4").unwrap();
        assert_eq!((f.to_string(), t.to_string(), d.to_string()), ("2026-10-01".into(), "2026-12-31".into(), "2027-01-31".into()));
        let (_, t, d) = quarter_bounds("2027-Q1").unwrap();
        assert_eq!((t.to_string(), d.to_string()), ("2027-03-31".into(), "2027-04-30".into()));
        assert!(quarter_bounds("2026-Q5").is_err());
    }

    #[test]
    fn a_quarter_with_sales_costs_and_reverse_charge() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        let cat_rep: String = c.query_row("SELECT id FROM categories WHERE vat_deductible = 0 LIMIT 1", [], |r| r.get(0)).unwrap();
        create(&mut c, &CLIENTS, &json!({"id": "c", "name": "Klant"})).unwrap();
        create(&mut c, &INVOICES, &json!({"id": "i", "client_id": "c", "date": "2026-10-15",
            "lines": [{"quantity_milli": 1000, "unit_cents": 125000, "vat_rate": "21"}]})).unwrap();
        crate::invoices::finalize(&mut c, "i").unwrap();
        create(&mut c, &INVOICES, &json!({"id": "eu", "client_id": "c", "date": "2026-11-01", "vat_scheme": "verlegd_eu",
            "lines": [{"quantity_milli": 1000, "unit_cents": 50000, "vat_rate": "21"}]})).unwrap();
        crate::invoices::finalize(&mut c, "eu").unwrap();
        // A concept: not counted, but warned about.
        create(&mut c, &INVOICES, &json!({"client_id": "c", "date": "2026-11-02"})).unwrap();
        create(&mut c, &DOCUMENTS, &json!({"date": "2026-10-03", "status": "bevestigd", "lines": [
            {"amount_excl_cents": 1000, "vat_rate": "21", "vat_cents": 210},
            {"amount_excl_cents": 2000, "vat_rate": "21", "vat_cents": 420, "business_pct": 50},
            {"amount_excl_cents": 5000, "vat_rate": "21", "vat_cents": 1050, "category_id": cat_rep},
            {"amount_excl_cents": 4000, "vat_rate": "verlegd_eu"}]})).unwrap();
        create(&mut c, &DOCUMENTS, &json!({"date": "2026-10-04", "status": "concept", "lines": [{"amount_excl_cents": 999, "vat_rate": "21", "vat_cents": 210}]})).unwrap();

        let rep = report(&c, "2026-Q4").unwrap();
        let get = |code: &str| rep.rubrieken.iter().find(|r| r.code == code).unwrap().clone();
        assert_eq!((get("1a").base_cents, get("1a").vat_cents), (125000, 26250));
        assert_eq!(get("3b").base_cents, 50000);
        assert_eq!((get("4b").base_cents, get("4b").vat_cents), (4000, 840));
        // 210 + half of 420 + nothing for representatie + 840 reverse charge.
        assert_eq!(get("5b").vat_cents, 210 + 210 + 840);
        // Every figure can be traced back: the sources add up to the rubriek.
        for code in ["1a", "3b", "4b", "5b"] {
            let r = get(code);
            assert!(!r.sources.is_empty(), "{code} has sources");
            assert_eq!(r.sources.iter().map(|x| x.vat_cents).sum::<i64>(), r.vat_cents, "{code} vat");
            if r.has_base {
                assert_eq!(r.sources.iter().map(|x| x.base_cents).sum::<i64>(), r.base_cents, "{code} base");
            }
        }
        assert_eq!(get("5a").vat_cents, 26250 + 840);
        assert_eq!(rep.total_cents, 26250 + 840 - 1260);
        assert_eq!(rep.total_eur, 262 + 8 - 13);
        assert_eq!(rep.deadline, "2027-01-31");
        assert_eq!(rep.warnings.len(), 2);
    }
}
