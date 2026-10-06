//! Reads a receipt or invoice from its text: date, total, btw per rate,
//! supplier, KvK and btw-id.
//!
//! The text comes from the phone's OCR (ML Kit) or from a PDF's text layer
//! (`pdftotext`). No model, only rules: what a Dutch kassabon or factuur
//! prints is regular enough, and the user confirms every draft anyway. What
//! the parser had to guess is said in `notes`, so the form can point at it.

use chrono::{Datelike, NaiveDate};
use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
pub struct DraftLine {
    pub vat_rate: String,
    pub amount_excl_cents: i64,
    pub vat_cents: i64,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct Draft {
    pub date: Option<String>,
    pub total_cents: Option<i64>,
    pub supplier_name: String,
    pub supplier_id: Option<String>,
    pub category_id: Option<String>,
    pub kvk: String,
    pub vat_id: String,
    pub lines: Vec<DraftLine>,
    pub notes: Vec<String>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// Every amount in a line, in cents: `1.234,56`, `12,34`, `12.34`, `€ 5,-`.
pub fn amounts(line: &str) -> Vec<i64> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r"(-?)(\d{1,3}(?:[.\s]\d{3})+|\d+)\s?([.,])\s?(\d{2}|-)(?:\b|$|\s)");
    let mut out = vec![];
    for c in r.captures_iter(line) {
        let int: String = c[2].chars().filter(char::is_ascii_digit).collect();
        let Ok(int) = int.parse::<i64>() else { continue };
        let frac = if &c[4] == "-" { 0 } else { c[4].parse::<i64>().unwrap_or(0) };
        // A thousands group written with the decimal sign ("1,234") is not an amount.
        let v = int * 100 + frac;
        out.push(if &c[1] == "-" { -v } else { v });
    }
    out
}

const MONTHS: &[(&str, u32)] = &[
    ("januari", 1), ("jan", 1), ("februari", 2), ("feb", 2), ("maart", 3), ("mrt", 3), ("mar", 3), ("april", 4), ("apr", 4),
    ("mei", 5), ("may", 5), ("juni", 6), ("jun", 6), ("juli", 7), ("jul", 7), ("augustus", 8), ("aug", 8),
    ("september", 9), ("sept", 9), ("sep", 9), ("oktober", 10), ("okt", 10), ("oct", 10), ("november", 11), ("nov", 11),
    ("december", 12), ("dec", 12), ("january", 1), ("february", 2), ("march", 3), ("june", 6), ("july", 7), ("august", 8),
    ("october", 10),
];

fn fix_year(y: i32) -> i32 {
    if y < 100 {
        2000 + y
    } else {
        y
    }
}

/// Every date in the text, in reading order.
pub fn dates(text: &str) -> Vec<NaiveDate> {
    static NUM: OnceLock<Regex> = OnceLock::new();
    static ISO: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    let num = re(&NUM, r"\b(\d{1,2})[-/.](\d{1,2})[-/.](\d{4}|\d{2})\b");
    let iso = re(&ISO, r"\b(\d{4})-(\d{2})-(\d{2})\b");
    let word = re(&WORD, r"(?i)\b(\d{1,2})\.?\s+([a-z]{3,9})\.?\s+(\d{4})\b");
    let mut found: Vec<(usize, NaiveDate)> = vec![];
    for c in iso.captures_iter(text) {
        if let Some(d) = NaiveDate::from_ymd_opt(c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0), c[3].parse().unwrap_or(0)) {
            found.push((c.get(0).unwrap().start(), d));
        }
    }
    for c in num.captures_iter(text) {
        let y = fix_year(c[3].parse().unwrap_or(0));
        if let Some(d) = NaiveDate::from_ymd_opt(y, c[2].parse().unwrap_or(0), c[1].parse().unwrap_or(0)) {
            found.push((c.get(0).unwrap().start(), d));
        }
    }
    for c in word.captures_iter(text) {
        let name = c[2].to_lowercase();
        if let Some((_, m)) = MONTHS.iter().find(|(n, _)| *n == name) {
            if let Some(d) = NaiveDate::from_ymd_opt(c[3].parse().unwrap_or(0), *m, c[1].parse().unwrap_or(0)) {
                found.push((c.get(0).unwrap().start(), d));
            }
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, d)| d).collect()
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// The total: an amount on a `totaal` / `te betalen` line beats one on a
/// `pin` / `betaald` line; the largest wins. Without either, the largest
/// amount in the text.
fn total(lines: &[&str]) -> (Option<i64>, bool) {
    static STRONG: OnceLock<Regex> = OnceLock::new();
    static WEAK: OnceLock<Regex> = OnceLock::new();
    let strong = re(&STRONG, r"(?i)\b(totaal|total|te betalen|totaalbedrag|amount due|bedrag incl|incl\.? btw|gesamtbetrag|rechnungsbetrag)\b");
    let weak = re(&WEAK, r"(?i)\b(pin|pinnen|betaald|bedrag|maestro|v pay|vpay|contactloos|debit|visa|mastercard|eur)\b");
    let sub = |l: &str| {
        let l = lower(l);
        l.contains("subtotaal") || l.contains("subtotal") || l.contains("excl") || l.contains("netto") || l.contains("btw ") && !l.contains("incl")
    };
    let on_or_after = |i: usize| -> Vec<i64> {
        let here = amounts(lines[i]);
        if here.is_empty() {
            lines.get(i + 1).map(|l| amounts(l)).unwrap_or_default()
        } else {
            here
        }
    };
    for kw in [strong, weak] {
        let best = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| kw.is_match(l) && !sub(l))
            .flat_map(|(i, _)| on_or_after(i))
            .filter(|v| *v > 0)
            .max();
        if best.is_some() {
            return (best, true);
        }
    }
    (lines.iter().flat_map(|l| amounts(l)).filter(|v| *v > 0).max(), false)
}

/// Btw per rate from lines that name the rate (`21%`, `9%`, `hoog`, `laag`).
fn vat_lines(lines: &[&str]) -> Vec<DraftLine> {
    static RATE: OnceLock<Regex> = OnceLock::new();
    let rate_re = re(&RATE, r"(?i)(?:\b(21|9|19|20|0)[,.]?0*\s?%|\b(hoog|laag)\b)");
    let mut out: Vec<DraftLine> = vec![];
    for (i, line) in lines.iter().enumerate() {
        let l = lower(line);
        if !(l.contains("btw") || l.contains("vat") || l.contains("mwst") || l.contains("ust") || l.contains("tax") || l.contains('%')) {
            continue;
        }
        let Some(c) = rate_re.captures(line) else { continue };
        let rate: i64 = match (c.get(1).map(|m| m.as_str()), c.get(2).map(|m| m.as_str().to_lowercase())) {
            (Some(r), _) => r.parse().unwrap_or(21),
            (None, Some(w)) if w == "laag" => 9,
            _ => 21,
        };
        if rate != 21 && rate != 9 {
            continue;
        }
        // The rate itself is not an amount.
        let rest = rate_re.replace_all(line, " ");
        let mut nums = amounts(&rest);
        if nums.is_empty() {
            nums = lines.get(i + 1).map(|n| amounts(n)).unwrap_or_default();
        }
        let nums: Vec<i64> = nums.into_iter().filter(|v| *v > 0).collect();
        let fits = |base: i64, vat: i64| (crate::tax::pct(base, rate) - vat).abs() <= 2;
        let mut pick = None;
        'outer: for &b in &nums {
            for &v in &nums {
                if b != v && fits(b, v) {
                    pick = Some((b, v));
                    break 'outer;
                }
                // An incl amount next to the btw: base = incl − btw.
                if b > v && fits(b - v, v) {
                    pick = Some((b - v, v));
                    break 'outer;
                }
            }
        }
        if pick.is_none() && nums.len() == 1 {
            // Only the btw amount: work the base back.
            let v = nums[0];
            pick = Some(((v as i128 * 100 / rate as i128) as i64, v));
        }
        if let Some((base, vat)) = pick {
            let r = rate.to_string();
            if !out.iter().any(|d| d.vat_rate == r && d.vat_cents == vat) {
                out.push(DraftLine { vat_rate: r, amount_excl_cents: base, vat_cents: vat });
            }
        }
    }
    out
}

fn ids(text: &str) -> (String, String) {
    static KVK: OnceLock<Regex> = OnceLock::new();
    static NLVAT: OnceLock<Regex> = OnceLock::new();
    static EUVAT: OnceLock<Regex> = OnceLock::new();
    let kvk = re(&KVK, r"(?i)\b(?:kvk|k\.v\.k\.|kamer van koophandel)[^0-9\n]{0,20}(\d{8})\b");
    let nl = re(&NLVAT, r"(?i)\b(NL\s?\d{9}\s?B\s?\d{2})\b");
    let eu = re(&EUVAT, r"(?i)(?:btw|vat|ust|tva|iva)[- ]?(?:id|nr|nummer|number|no|reg)?[.:\s#-]{0,6}([A-Z]{2}\s?[0-9A-Z]{8,12})\b");
    let k = kvk.captures(text).map(|c| c[1].to_string()).unwrap_or_default();
    let v = nl
        .captures(text)
        .or_else(|| eu.captures(text))
        .map(|c| c[1].to_uppercase().replace(' ', ""))
        .unwrap_or_default();
    (k, v)
}

fn first_name_line(lines: &[&str]) -> String {
    let skip = ["kassabon", "bon", "factuur", "invoice", "rechnung", "welkom", "receipt", "kopie", "duplicaat"];
    lines
        .iter()
        .map(|l| l.trim())
        .find(|l| {
            let letters = l.chars().filter(|c| c.is_alphabetic()).count();
            letters >= 3 && amounts(l).is_empty() && dates(l).is_empty() && !skip.contains(&lower(l).as_str())
        })
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect()
}

/// Parses `text`. `today` bounds the date (no future dates, none older than
/// two years); `conn`, when given, matches the supplier against those known.
pub fn parse(text: &str, today: NaiveDate, conn: Option<&Connection>) -> Draft {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut d = Draft::default();

    let oldest = today.with_year(today.year() - 2).unwrap_or(today);
    d.date = dates(text).into_iter().find(|x| *x <= today && *x >= oldest).map(|x| x.to_string());
    if d.date.is_none() {
        d.notes.push("Geen datum gevonden".into());
    }

    let (tot, sure) = total(&lines);
    d.total_cents = tot;
    if tot.is_none() {
        d.notes.push("Geen totaalbedrag gevonden".into());
    } else if !sure {
        d.notes.push("Totaal geraden: het grootste bedrag op de bon".into());
    }

    let (kvk, vat_id) = ids(text);
    d.kvk = kvk;
    d.vat_id = vat_id;

    let l = lower(text);
    let reverse = ["reverse charge", "verlegd", "steuerschuldnerschaft", "autoliquidation", "vat reverse", "inversione contabile"]
        .iter()
        .any(|k| l.contains(k));
    let foreign_vat = !d.vat_id.is_empty() && !d.vat_id.starts_with("NL");
    d.lines = vat_lines(&lines);
    if reverse || (d.lines.is_empty() && foreign_vat) {
        let total = d.total_cents.unwrap_or(0);
        d.lines = vec![DraftLine { vat_rate: "verlegd_eu".into(), amount_excl_cents: total, vat_cents: 0 }];
        d.notes.push("Btw verlegd (buitenlandse leverancier): komt in rubriek 4b; controleer of het buiten de EU (4a) is".into());
    } else if d.lines.is_empty() {
        if let Some(total) = d.total_cents {
            let base = (total as i128 * 100 * 2 + 121) / (121 * 2);
            let base = base as i64;
            d.lines = vec![DraftLine { vat_rate: "21".into(), amount_excl_cents: base, vat_cents: total - base }];
            d.notes.push("Geen btw op de bon gevonden: 21% aangenomen".into());
        }
    } else if let Some(total) = d.total_cents {
        let sum: i64 = d.lines.iter().map(|x| x.amount_excl_cents + x.vat_cents).sum();
        if (sum - total).abs() > 2 {
            if sum < total {
                // The rest has no btw on it (statiegeld, a 0% item) or was missed.
                d.lines.push(DraftLine { vat_rate: "0".into(), amount_excl_cents: total - sum, vat_cents: 0 });
            }
            d.notes.push("Btw-regels tellen niet op tot het totaal; controleer de bedragen".into());
        }
    }

    // Supplier.
    if let Some(conn) = conn {
        if let Some((id, name, cat, rate)) = match_supplier(conn, &l, &d.kvk, &d.vat_id) {
            d.supplier_id = Some(id);
            d.supplier_name = name;
            d.category_id = cat;
            if !rate.is_empty() && d.lines.len() == 1 && d.notes.iter().any(|n| n.contains("aangenomen")) {
                d.lines[0].vat_rate = rate;
            }
        }
    }
    if d.supplier_name.is_empty() {
        d.supplier_name = first_name_line(&lines);
    }
    d
}

/// A known supplier whose KvK, btw-id or one of its match terms appears.
fn match_supplier(conn: &Connection, text: &str, kvk: &str, vat: &str) -> Option<(String, String, Option<String>, String)> {
    let mut stmt = conn.prepare("SELECT id, name, kvk, vat_id, match_terms, default_category_id, default_vat_rate FROM suppliers").ok()?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .ok()?;
    let mut best: Option<(usize, (String, String, Option<String>, String))> = None;
    for (id, name, s_kvk, s_vat, terms, cat, rate) in rows.flatten() {
        let score = if !kvk.is_empty() && s_kvk == kvk || !vat.is_empty() && s_vat == vat {
            1000
        } else {
            terms.split(',').map(str::trim).filter(|t| t.len() >= 3 && text.contains(*t)).map(str::len).max().unwrap_or(0)
        };
        if score > 0 && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, (id, name, cat, rate)));
        }
    }
    best.map(|(_, b)| b)
}

/// After the user confirms a document: remember the supplier, its first
/// category and a match term, so the next receipt from it fills itself in.
pub fn learn(conn: &Connection, doc: &serde_json::Value) -> rusqlite::Result<Option<String>> {
    let name = doc["supplier_name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }
    let category = doc["lines"][0]["category_id"].as_str().map(str::to_string);
    let rate = doc["lines"][0]["vat_rate"].as_str().unwrap_or("").to_string();
    let term = name.to_lowercase();
    let now = crate::db::now();
    let existing: Option<(String, String)> = match doc["supplier_id"].as_str() {
        Some(id) => conn.query_row("SELECT id, match_terms FROM suppliers WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok(),
        None => conn
            .query_row("SELECT id, match_terms FROM suppliers WHERE lower(name) = ?1", [&term], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok(),
    };
    let id = match existing {
        Some((id, terms)) => {
            let mut list: Vec<String> = terms.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            if !list.contains(&term) {
                list.push(term);
            }
            conn.execute(
                "UPDATE suppliers SET match_terms = ?1, default_category_id = COALESCE(?2, default_category_id), default_vat_rate = ?3, updated_at = ?4 WHERE id = ?5",
                rusqlite::params![list.join(","), category, rate, now, id],
            )?;
            id
        }
        None => {
            let id = crate::db::new_id();
            conn.execute(
                "INSERT INTO suppliers (id, name, match_terms, default_category_id, default_vat_rate, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                rusqlite::params![id, name, term, category, rate, now],
            )?;
            id
        }
    };
    if doc["supplier_id"].is_null() {
        conn.execute("UPDATE documents SET supplier_id = ?1 WHERE id = ?2", [&id, doc["id"].as_str().unwrap_or("")])?;
    }
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
    }

    #[test]
    fn amounts_in_dutch_and_english_notation() {
        assert_eq!(amounts("Totaal 1.234,56"), vec![123456]);
        assert_eq!(amounts("TOTAL € 12.34 EUR"), vec![1234]);
        assert_eq!(amounts("2 x 3,99  7,98"), vec![399, 798]);
        assert_eq!(amounts("Korting -1,00"), vec![-100]);
        assert_eq!(amounts("Prijs 5,-"), vec![500]);
        assert_eq!(amounts("Tel 035 123 4567"), Vec::<i64>::new());
    }

    #[test]
    fn dates_in_every_common_shape() {
        let d = |s: &str| dates(s).first().map(|d| d.to_string());
        assert_eq!(d("Datum: 03-10-2026 14:22"), Some("2026-10-03".into()));
        assert_eq!(d("3/10/26"), Some("2026-10-03".into()));
        assert_eq!(d("2026-10-03"), Some("2026-10-03".into()));
        assert_eq!(d("Factuurdatum 3 oktober 2026"), Some("2026-10-03".into()));
        assert_eq!(d("Invoice date: 01 Oct 2026"), Some("2026-10-01".into()));
    }

    #[test]
    fn an_albert_heijn_kassabon() {
        let text = "Albert Heijn\nKoningsstraat 1 Hilversum\nAANTAL OMSCHRIJVING PRIJS BEDRAG\n1 KOFFIE 4,99\n1 BRIEFPAPIER 6,50\nSUBTOTAAL 11,49\nTOTAAL 11,49\nBETAALD MET:\nPINNEN 11,49\nBTW OVER EUR\n9% 4,58 0,41\n21% 5,37 1,13\n03-10-2026 12:01 KvK 35012085";
        let d = parse(text, today(), None);
        assert_eq!(d.total_cents, Some(1149));
        assert_eq!(d.date.as_deref(), Some("2026-10-03"));
        assert_eq!(d.supplier_name, "Albert Heijn");
        assert_eq!(d.kvk, "35012085");
        assert_eq!(
            d.lines,
            vec![
                DraftLine { vat_rate: "9".into(), amount_excl_cents: 458, vat_cents: 41 },
                DraftLine { vat_rate: "21".into(), amount_excl_cents: 537, vat_cents: 113 },
            ]
        );
        assert!(d.notes.is_empty(), "{:?}", d.notes);
    }

    #[test]
    fn a_hetzner_invoice_is_reverse_charged() {
        let text = "Hetzner Online GmbH\nIndustriestr. 25\nInvoice no. 081234\nInvoice date: 01 Oct 2026\nVAT ID DE812871812\nTotal 4,51 €\nReverse charge: Tax liability of the recipient";
        let d = parse(text, today(), None);
        assert_eq!(d.total_cents, Some(451));
        assert_eq!(d.vat_id, "DE812871812");
        assert_eq!(d.lines[0].vat_rate, "verlegd_eu");
        assert_eq!(d.lines[0].amount_excl_cents, 451);
    }

    #[test]
    fn a_receipt_without_btw_assumes_21_and_says_so() {
        let d = parse("Parkeergarage\n05-10-2026\nTe betalen 12,10", today(), None);
        assert_eq!(d.lines, vec![DraftLine { vat_rate: "21".into(), amount_excl_cents: 1000, vat_cents: 210 }]);
        assert!(d.notes.iter().any(|n| n.contains("21% aangenomen")));
    }

    #[test]
    fn future_dates_are_not_the_receipt_date() {
        let d = parse("Winkel\nGeldig tot 01-01-2027\nDatum 02-10-2026\nTotaal 1,00", today(), None);
        assert_eq!(d.date.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn a_confirmed_supplier_is_recognised_next_time() {
        let pool = crate::db::memory();
        let c = pool.get().unwrap();
        let cat: String = c.query_row("SELECT id FROM categories LIMIT 1", [], |r| r.get(0)).unwrap();
        c.execute("INSERT INTO documents (id, date, created_at, updated_at) VALUES ('d', '2026-10-01', 'x', 'x')", []).unwrap();
        let doc = serde_json::json!({"id": "d", "supplier_name": "Coolblue", "supplier_id": null, "lines": [{"category_id": cat, "vat_rate": "21"}]});
        learn(&c, &doc).unwrap();
        let d = parse("COOLBLUE B.V.\n01-10-2026\nTotaal 121,00\nBTW 21% 100,00 21,00", today(), Some(&c));
        assert_eq!(d.supplier_name, "Coolblue");
        assert_eq!(d.category_id.as_deref(), Some(cat.as_str()));
    }
}
