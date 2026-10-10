//! The year's figures for the inkomstenbelasting: winst, ondernemersaftrek,
//! MKB-winstvrijstelling, KIA, urencriterium, and the year-end balance.

use crate::db;
use crate::entities::{self, ASSETS, DOCUMENTS, HOURS, TRIPS};
use crate::error::ApiResult;
use crate::invoices;
use crate::tax::{self, bp, pct};
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Serialize, Debug)]
pub struct AssetYear {
    pub id: String,
    pub name: String,
    pub depreciation_cents: i64,
    pub book_value_end_cents: i64,
}

#[derive(Serialize, Debug)]
pub struct IbReport {
    pub year: i32,
    pub table_year: i32,
    pub table_source: &'static str,
    pub omzet_cents: i64,
    /// Costs per IB cost group, after business share and deductible share.
    pub kosten: BTreeMap<String, i64>,
    pub kosten_cents: i64,
    pub afschrijvingen: Vec<AssetYear>,
    pub afschrijving_cents: i64,
    pub km: f64,
    pub km_cents: i64,
    pub winst_cents: i64,
    pub investeringen_cents: i64,
    pub kia_cents: i64,
    pub uren: f64,
    pub uren_nodig: i64,
    pub urencriterium: bool,
    pub zelfstandigenaftrek_cents: i64,
    pub startersaftrek_cents: i64,
    pub mkb_vrijstelling_cents: i64,
    pub belastbare_winst_cents: i64,
    pub indicatie_box1_cents: i64,
    pub balans: Balans,
    pub deadline: String,
    pub warnings: Vec<String>,
    pub status: String,
}

#[derive(Serialize, Debug, Default)]
pub struct Balans {
    pub activa_cents: i64,
    pub debiteuren_cents: i64,
}

/// Months an asset is in use in `year` and before it, counting the month of
/// purchase as a whole month and stopping at the month it was disposed of.
fn months(purchase: NaiveDate, disposed: Option<NaiveDate>, year: i32) -> (i64, i64) {
    let idx = |d: NaiveDate| d.year() as i64 * 12 + d.month0() as i64;
    let start = idx(purchase);
    let end = disposed.map(idx).unwrap_or(i64::MAX);
    let (ys, ye) = (year as i64 * 12, year as i64 * 12 + 12);
    let before = (ys.min(end.saturating_add(1)) - start).max(0);
    let during = (ye.min(end.saturating_add(1)) - ys.max(start)).max(0);
    (before, during.min(12))
}

pub fn depreciation(cost: i64, residual: i64, years: i64, purchase: NaiveDate, disposed: Option<NaiveDate>, year: i32) -> (i64, i64) {
    let total = (cost - residual).max(0);
    let lifetime = years.max(1) * 12;
    let (before, during) = months(purchase, disposed, year);
    let done = (total as i128 * before.min(lifetime) as i128 / lifetime as i128) as i64;
    let upto = (total as i128 * (before + during).min(lifetime) as i128 / lifetime as i128) as i64;
    let dep = upto - done;
    (dep, cost - upto)
}

fn date(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

pub fn report(conn: &Connection, year: i32) -> ApiResult<IbReport> {
    let (t, exact) = tax::year(year);
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut warnings = vec![];
    if !exact {
        warnings.push(format!("Geen belastingtabel voor {year}; de bedragen van {} zijn gebruikt", t.year));
    }

    let mut omzet = 0;
    let mut debiteuren = 0;
    for inv in invoices::final_between(conn, &from, &to)? {
        let tot = invoices::totals_of(&inv);
        omzet += tot.excl_cents;
        let paid_in_year = inv["paid_at"].as_str().map(|p| p <= to.as_str()).unwrap_or(false);
        if !paid_in_year {
            debiteuren += tot.incl_cents;
        }
    }

    // Costs.
    let cats: HashMap<String, (String, i64, bool)> = {
        let mut stmt = conn.prepare("SELECT id, ib_group, deductible_pct, vat_deductible FROM categories")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0))))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut kosten: BTreeMap<String, i64> = BTreeMap::new();
    let mut uncategorised = 0;
    let mut concept = 0;
    let q = entities::ListQuery { from: Some(from.clone()), to: Some(to.clone()), ..Default::default() };
    for doc in entities::list(conn, &DOCUMENTS, &q)? {
        if doc["status"] != "bevestigd" {
            concept += 1;
            continue;
        }
        for line in doc["lines"].as_array().into_iter().flatten() {
            let cat = line["category_id"].as_str().and_then(|c| cats.get(c));
            if cat.is_none() {
                uncategorised += 1;
            }
            let (group, deductible_pct, vat_deductible) = cat.cloned().unwrap_or(("overig".into(), 100, true));
            if group == "investering" {
                continue;
            }
            let base = line["amount_excl_cents"].as_i64().unwrap_or(0);
            let rate = line["vat_rate"].as_str().unwrap_or("");
            let lost_vat = match (vat_deductible, rate) {
                (true, _) => 0,
                (false, "21" | "9") => line["vat_cents"].as_i64().unwrap_or(0),
                (false, "verlegd_eu" | "verlegd_buiten_eu") => pct(base, 21),
                _ => 0,
            };
            let business = pct(base + lost_vat, line["business_pct"].as_i64().unwrap_or(100));
            *kosten.entry(group).or_default() += pct(business, deductible_pct);
        }
    }
    if concept > 0 {
        warnings.push(if concept == 1 { format!("1 kostenpost in {year} is nog niet bevestigd en telt niet mee") } else { format!("{concept} kostenposten in {year} zijn nog niet bevestigd en tellen niet mee") });
    }
    if uncategorised > 0 {
        warnings.push(if uncategorised == 1 { "1 kostenregel zonder categorie staat onder andere kosten".to_string() } else { format!("{uncategorised} kostenregels zonder categorie staan onder andere kosten") });
    }
    let kosten_cents: i64 = kosten.values().sum();

    // Assets.
    let mut afschrijvingen = vec![];
    let mut investeringen = 0;
    let mut activa = 0;
    for a in entities::list(conn, &ASSETS, &Default::default())? {
        let Some(purchase) = date(&a["purchase_date"]) else { continue };
        if purchase.year() > year {
            continue;
        }
        let cost = a["cost_cents"].as_i64().unwrap_or(0);
        let disposed = date(&a["disposed_date"]);
        let (dep, book) = depreciation(cost, a["residual_cents"].as_i64().unwrap_or(0), a["useful_years"].as_i64().unwrap_or(5), purchase, disposed, year);
        let business = a["business_pct"].as_i64().unwrap_or(100);
        if purchase.year() == year && cost >= t.kia_item_min {
            investeringen += pct(cost, business);
        }
        if disposed.map(|d| d.year() <= year).unwrap_or(false) && dep == 0 {
            continue;
        }
        activa += book;
        afschrijvingen.push(AssetYear {
            id: a["id"].as_str().unwrap_or_default().into(),
            name: a["name"].as_str().unwrap_or_default().into(),
            depreciation_cents: pct(dep, business),
            book_value_end_cents: book,
        });
    }
    let afschrijving_cents: i64 = afschrijvingen.iter().map(|a| a.depreciation_cents).sum();

    // Kilometres.
    let mut km_tenths = 0;
    for trip in entities::list(conn, &TRIPS, &q)? {
        let k = trip["km_tenths"].as_i64().unwrap_or(0);
        km_tenths += if trip["return_trip"] == true { 2 * k } else { k };
    }
    let km_cents = (km_tenths * t.km_cents + 5) / 10;

    let minutes: i64 = entities::list(conn, &HOURS, &q)?.iter().map(|h| h["minutes"].as_i64().unwrap_or(0)).sum();
    let urencriterium = minutes >= t.uren * 60;

    let winst = omzet - kosten_cents - afschrijving_cents - km_cents;
    let kia = t.kia(investeringen);
    let winst_na_kia = winst - kia;

    let starter = db::setting(conn, "startersaftrek").map(|v| v == "ja").unwrap_or(true);
    let (mut za, sa) = if urencriterium { (t.zelfstandigenaftrek, if starter { t.startersaftrek } else { 0 }) } else { (0, 0) };
    if !starter {
        // Without startersaftrek the zelfstandigenaftrek cannot exceed the winst.
        za = za.min(winst_na_kia.max(0));
    }
    let na_oa = winst_na_kia - za - sa;
    let mkb = bp(na_oa, t.mkb_bp);
    let belastbaar = na_oa - mkb;
    if !urencriterium {
        warnings.push(format!(
            "Urencriterium nog niet gehaald ({:.0} van {} uur): zonder 1.225 uur geen zelfstandigen- en startersaftrek",
            minutes as f64 / 60.0,
            t.uren
        ));
    }

    let status = conn
        .query_row("SELECT status FROM periods WHERE kind = 'ib_year' AND key = ?1", [year.to_string()], |r| r.get(0))
        .unwrap_or_else(|_| "open".to_string());
    Ok(IbReport {
        year,
        table_year: t.year,
        table_source: t.source,
        omzet_cents: omzet,
        kosten,
        kosten_cents,
        afschrijvingen,
        afschrijving_cents,
        km: km_tenths as f64 / 10.0,
        km_cents,
        winst_cents: winst,
        investeringen_cents: investeringen,
        kia_cents: kia,
        uren: minutes as f64 / 60.0,
        uren_nodig: t.uren,
        urencriterium,
        zelfstandigenaftrek_cents: za,
        startersaftrek_cents: sa,
        mkb_vrijstelling_cents: mkb,
        belastbare_winst_cents: belastbaar,
        indicatie_box1_cents: t.box1_tax(belastbaar.max(0)),
        balans: Balans { activa_cents: activa, debiteuren_cents: debiteuren },
        deadline: format!("{}-05-01", year + 1),
        warnings,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{create, CLIENTS, INVOICES};
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn depreciation_is_linear_by_month() {
        // €1.200 laptop, 3 years, bought in October: 3 of 36 months in year one.
        assert_eq!(depreciation(120000, 0, 3, d("2026-10-15"), None, 2026), (10000, 110000));
        assert_eq!(depreciation(120000, 0, 3, d("2026-10-15"), None, 2027), (40000, 70000));
        assert_eq!(depreciation(120000, 0, 3, d("2026-10-15"), None, 2029), (30000, 0));
        assert_eq!(depreciation(120000, 0, 3, d("2026-10-15"), None, 2030), (0, 0));
        assert_eq!(depreciation(120000, 0, 3, d("2026-10-15"), Some(d("2027-02-10")), 2027), (6666, 110000 - 6666));
    }

    #[test]
    fn a_year_from_omzet_to_belastbare_winst() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        create(&mut c, &CLIENTS, &json!({"id": "c", "name": "Klant"})).unwrap();
        create(&mut c, &INVOICES, &json!({"id": "i", "client_id": "c", "date": "2026-11-01",
            "lines": [{"quantity_milli": 1000, "unit_cents": 2000000, "vat_rate": "21"}]})).unwrap();
        crate::invoices::finalize(&mut c, "i").unwrap();
        let rep_cat: String = c.query_row("SELECT id FROM categories WHERE deductible_pct = 80", [], |r| r.get(0)).unwrap();
        create(&mut c, &DOCUMENTS, &json!({"date": "2026-10-03", "status": "bevestigd", "lines": [
            {"amount_excl_cents": 100000, "vat_rate": "21", "vat_cents": 21000},
            {"amount_excl_cents": 10000, "vat_rate": "21", "vat_cents": 2100, "category_id": rep_cat}]})).unwrap();
        create(&mut c, &ASSETS, &json!({"name": "Laptop", "purchase_date": "2026-10-01", "cost_cents": 300000, "useful_years": 5})).unwrap();
        create(&mut c, &TRIPS, &json!({"date": "2026-10-05", "km_tenths": 1000, "return_trip": true})).unwrap();
        create(&mut c, &HOURS, &json!({"date": "2026-10-05", "minutes": 1225 * 60})).unwrap();

        let r = report(&c, 2026).unwrap();
        assert_eq!(r.omzet_cents, 2000000);
        // 1.000 + 80% of (100 + 21 btw not deductible).
        assert_eq!(r.kosten_cents, 100000 + 9680);
        assert_eq!(r.afschrijving_cents, 15000);
        assert_eq!(r.km_cents, 200 * 23);
        let winst = 2000000 - 109680 - 15000 - 4600;
        assert_eq!(r.winst_cents, winst);
        assert_eq!(r.kia_cents, 84000);
        assert!(r.urencriterium);
        let na = winst - 84000 - 120000 - 212300;
        assert_eq!(r.belastbare_winst_cents, na - bp(na, 1270));
        assert_eq!(r.balans.activa_cents, 285000);
        assert_eq!(r.balans.debiteuren_cents, 2420000);
    }
}
