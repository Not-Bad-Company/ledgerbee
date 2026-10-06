//! Bank statements: CSV from the Dutch banks, or CAMT.053 XML, and matching
//! the transactions to invoices and documents.
//!
//! CSV columns are found by their header names rather than by bank, so a bank
//! that renames or reorders a column still imports. ABN AMRO's headerless
//! TXT export is recognised by its shape.

use crate::db;
use crate::error::{ApiError, ApiResult};
use chrono::NaiveDate;
use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tx {
    pub date: String,
    pub amount_cents: i64,
    pub counterparty: String,
    pub iban: String,
    pub description: String,
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    for f in ["%Y-%m-%d", "%d-%m-%Y", "%Y%m%d", "%d/%m/%Y", "%d.%m.%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            return Some(d);
        }
    }
    None
}

/// `-12,34`, `+1.234,56`, `12.34`, `−5,00` → cents.
fn parse_amount(s: &str) -> Option<i64> {
    let s: String = s.trim().replace('−', "-").chars().filter(|c| !c.is_whitespace() && *c != '€').collect();
    let neg = s.starts_with('-');
    let s = s.trim_start_matches(['-', '+']);
    let (int, frac) = match (s.rfind(','), s.rfind('.')) {
        (Some(c), Some(d)) if c > d => (s[..c].replace('.', ""), &s[c + 1..]),
        (Some(_), Some(d)) => (s[..d].replace(',', ""), &s[d + 1..]),
        (Some(c), None) if s.len() - c - 1 <= 2 => (s[..c].to_string(), &s[c + 1..]),
        (None, Some(d)) if s.len() - d - 1 <= 2 => (s[..d].to_string(), &s[d + 1..]),
        _ => (s.replace(['.', ','], ""), ""),
    };
    let frac = format!("{frac:0<2}");
    let v = int.parse::<i64>().ok()? * 100 + frac[..2].parse::<i64>().ok()?;
    Some(if neg { -v } else { v })
}

fn find(headers: &[String], names: &[&str]) -> Option<usize> {
    names.iter().find_map(|n| headers.iter().position(|h| h == n))
}

pub fn parse_csv(data: &str) -> ApiResult<Vec<Tx>> {
    let data = data.trim_start_matches('\u{feff}');
    let first = data.lines().next().unwrap_or_default();
    let delim = [b';', b'\t', b','].into_iter().max_by_key(|d| first.matches(*d as char).count()).unwrap_or(b',');
    let mut rdr = csv::ReaderBuilder::new().delimiter(delim).has_headers(false).flexible(true).from_reader(data.as_bytes());
    let mut rows = rdr.records().filter_map(Result::ok);
    let Some(head) = rows.next() else { return Ok(vec![]) };
    let headers: Vec<String> = head.iter().map(|h| h.trim().to_lowercase()).collect();

    // ABN AMRO TXT: no header; account, currency, date, start, end, valuedate, amount, description.
    if parse_date(&headers.get(2).cloned().unwrap_or_default()).is_some() && headers.len() >= 8 && delim == b'\t' {
        let mut out = vec![];
        for r in std::iter::once(head).chain(rows) {
            let (Some(d), Some(a)) = (parse_date(&r[2]), parse_amount(&r[6])) else { continue };
            out.push(Tx { date: d.to_string(), amount_cents: a, counterparty: String::new(), iban: String::new(), description: r[7].trim().to_string() });
        }
        return Ok(out);
    }

    let date = find(&headers, &["datum", "date", "transactiedatum", "boekingsdatum", "rentedatum", "interest date"])
        .ok_or_else(|| ApiError::bad("Geen datumkolom gevonden in dit bestand"))?;
    let amount = find(&headers, &["bedrag (eur)", "bedrag", "amount", "transactiebedrag", "amount (eur)"])
        .ok_or_else(|| ApiError::bad("Geen bedragkolom gevonden in dit bestand"))?;
    let sign = find(&headers, &["af bij", "af/bij", "creditdebet", "credit/debit", "debit/credit"]);
    let name = find(&headers, &["naam tegenpartij", "naam / omschrijving", "tegenrekeninghouder", "name", "naam", "counterparty name", "description of counterparty"]);
    let iban = find(&headers, &["tegenrekening iban/bban", "tegenrekening", "tegenrekeningnummer", "counterparty", "iban tegenpartij", "counterparty iban"]);
    let desc: Vec<usize> = headers
        .iter()
        .enumerate()
        .filter(|(_, h)| h.starts_with("omschrijving") || *h == "mededelingen" || *h == "description" || *h == "betalingskenmerk")
        .map(|(i, _)| i)
        .collect();

    let mut out = vec![];
    for r in rows {
        let get = |i: usize| r.get(i).unwrap_or("").trim().to_string();
        let (Some(d), Some(mut a)) = (parse_date(&get(date)), parse_amount(&get(amount))) else { continue };
        if let Some(s) = sign {
            let s = get(s).to_lowercase();
            if s == "af" || s == "d" || s == "debet" || s == "debit" {
                a = -a.abs();
            } else if s == "bij" || s == "c" || s == "credit" {
                a = a.abs();
            }
        }
        let description = desc.iter().map(|i| get(*i)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        out.push(Tx {
            date: d.to_string(),
            amount_cents: a,
            counterparty: name.map(get).unwrap_or_default(),
            iban: iban.map(get).unwrap_or_default(),
            description,
        });
    }
    Ok(out)
}

/// CAMT.053: one `<Ntry>` per transaction.
pub fn parse_camt(xml: &str) -> ApiResult<Vec<Tx>> {
    let entry = Regex::new(r"(?s)<Ntry>(.*?)</Ntry>").expect("regex");
    let tag = |block: &str, path: &str| -> String {
        // The first <tag> whose name ends the path, inside the previous ones.
        let mut scope = block.to_string();
        for t in path.split('/') {
            let r = Regex::new(&format!(r"(?s)<{t}(?:\s[^>]*)?>(.*?)</{t}>")).expect("regex");
            match r.captures(&scope) {
                Some(c) => scope = c[1].to_string(),
                None => return String::new(),
            }
        }
        scope.trim().to_string()
    };
    let mut out = vec![];
    for c in entry.captures_iter(xml) {
        let b = &c[1];
        let Some(amount) = parse_amount(&tag(b, "Amt")) else { continue };
        let debit = tag(b, "CdtDbtInd") == "DBIT";
        let date = parse_date(&tag(b, "BookgDt/Dt")).or_else(|| parse_date(&tag(b, "BookgDt/DtTm").chars().take(10).collect::<String>()));
        let Some(date) = date else { continue };
        let party = if debit { "Cdtr" } else { "Dbtr" };
        let account = if debit { "CdtrAcct" } else { "DbtrAcct" };
        out.push(Tx {
            date: date.to_string(),
            amount_cents: if debit { -amount.abs() } else { amount.abs() },
            counterparty: tag(b, &format!("RltdPties/{party}/Nm")),
            iban: tag(b, &format!("RltdPties/{account}/Id/IBAN")),
            description: tag(b, "RmtInf/Ustrd"),
        });
    }
    if out.is_empty() && !xml.contains("<Ntry>") {
        return Err(ApiError::bad("Geen CAMT.053-bestand"));
    }
    Ok(out)
}

pub fn parse_any(bytes: &[u8]) -> ApiResult<Vec<Tx>> {
    let text = String::from_utf8_lossy(bytes);
    if text.trim_start().starts_with('<') {
        parse_camt(&text)
    } else {
        parse_csv(&text)
    }
}

#[derive(Serialize, Debug, Default)]
pub struct ImportResult {
    pub added: usize,
    pub duplicates: usize,
    pub matched: usize,
}

/// Stores new transactions (a transaction seen before is skipped by its hash)
/// and matches what can be matched without asking.
pub fn import(conn: &mut Connection, txs: &[Tx]) -> ApiResult<ImportResult> {
    let mut res = ImportResult::default();
    let tx = conn.transaction()?;
    let now = db::now();
    // Two identical rows on one day (two coffees) are both real: number them.
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    for t in txs {
        let base = format!("{}|{}|{}|{}|{}", t.date, t.amount_cents, t.iban, t.counterparty, t.description);
        let n = seen.entry(base.clone()).or_default();
        *n += 1;
        let hash = hex::encode(Sha256::digest(format!("{base}|{n}")));
        let added = tx.execute(
            "INSERT OR IGNORE INTO bank_transactions (id, date, amount_cents, counterparty, iban, description, import_hash, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![db::new_id(), t.date, t.amount_cents, t.counterparty, t.iban, t.description, hash, now],
        )?;
        if added == 1 {
            res.added += 1;
        } else {
            res.duplicates += 1;
        }
    }
    tx.commit()?;
    res.matched = auto_match(conn)?;
    Ok(res)
}

#[derive(Serialize, Debug, Clone)]
pub struct Suggestion {
    pub transaction_id: String,
    pub kind: String,
    pub id: String,
    pub label: String,
    pub score: i64,
}

/// Incoming money that names an open invoice's number and has its exact total
/// marks that invoice paid without asking. Everything else is a suggestion.
pub fn auto_match(conn: &mut Connection) -> ApiResult<usize> {
    let open: Vec<(String, String, i64)> = open_invoices(conn)?;
    let incoming: Vec<(String, String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, date, amount_cents, description || ' ' || counterparty FROM bank_transactions WHERE matched_id = '' AND ignored = 0 AND amount_cents > 0",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut n = 0;
    for (tid, date, amount, text) in incoming {
        if let Some((iid, _, _)) = open.iter().find(|(_, number, total)| *total == amount && text.contains(number.as_str())) {
            link(conn, &tid, "invoice", iid)?;
            crate::invoices::set_paid(conn, iid, Some(&date))?;
            n += 1;
        }
    }
    Ok(n)
}

fn open_invoices(conn: &Connection) -> ApiResult<Vec<(String, String, i64)>> {
    let mut out = vec![];
    for inv in crate::entities::list(conn, &crate::entities::INVOICES, &Default::default())? {
        if inv["status"] == "verzonden" {
            let total = crate::invoices::totals_of(&inv).incl_cents;
            out.push((inv["id"].as_str().unwrap_or("").into(), inv["number"].as_str().unwrap_or("").into(), total));
        }
    }
    Ok(out)
}

pub fn link(conn: &Connection, transaction: &str, kind: &str, id: &str) -> ApiResult<()> {
    let n = conn.execute(
        "UPDATE bank_transactions SET matched_kind = ?1, matched_id = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![kind, id, db::now(), transaction],
    )?;
    if n == 0 {
        return Err(ApiError::not_found());
    }
    crate::entities::audit(conn, "bank_transactions", transaction, "match", None, Some(&serde_json::json!({"kind": kind, "id": id})))?;
    Ok(())
}

/// For each unmatched transaction, the best candidate: an open invoice with
/// the same total, or a document with the same total within ten days, ranked
/// up when the supplier's name or match terms appear in the bank line.
pub fn suggestions(conn: &Connection) -> ApiResult<Vec<Suggestion>> {
    let open = open_invoices(conn)?;
    let docs: Vec<(String, String, i64, String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT d.id, d.date, d.total_cents, lower(d.supplier_name), lower(COALESCE(s.match_terms, '')), d.supplier_name FROM documents d
             LEFT JOIN suppliers s ON s.id = d.supplier_id
             WHERE d.id NOT IN (SELECT matched_id FROM bank_transactions WHERE matched_kind = 'document') AND d.paid_private = 0",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let txs: Vec<(String, String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, date, amount_cents, lower(counterparty || ' ' || description) FROM bank_transactions WHERE matched_id = '' AND ignored = 0",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let days = |a: &str, b: &str| -> i64 {
        match (parse_date(a), parse_date(b)) {
            (Some(a), Some(b)) => (a - b).num_days().abs(),
            _ => i64::MAX,
        }
    };
    let mut out = vec![];
    for (tid, date, amount, text) in txs {
        let mut best: Option<Suggestion> = None;
        let mut consider = |s: Suggestion| {
            if best.as_ref().map(|b| s.score > b.score).unwrap_or(true) {
                best = Some(s);
            }
        };
        if amount > 0 {
            for (iid, number, total) in &open {
                if *total == amount {
                    let score = 50 + if text.contains(&number.to_lowercase()) { 50 } else { 0 };
                    consider(Suggestion { transaction_id: tid.clone(), kind: "invoice".into(), id: iid.clone(), label: format!("Factuur {number}"), score });
                }
            }
        } else {
            for (did, ddate, total, name, terms, shown) in &docs {
                let gap = days(&date, ddate);
                if (*total + amount).abs() > 1 || gap > 10 {
                    continue;
                }
                let named = (!name.is_empty() && name.split_whitespace().next().map(|w| w.len() >= 3 && text.contains(w)).unwrap_or(false))
                    || terms.split(',').any(|t| t.len() >= 3 && text.contains(t.trim()));
                let score = 60 - gap * 2 + if named { 40 } else { 0 };
                consider(Suggestion { transaction_id: tid.clone(), kind: "document".into(), id: did.clone(), label: format!("{} ({ddate})", if shown.is_empty() { "Bon" } else { shown }), score });
            }
        }
        if let Some(b) = best {
            out.push(b);
        }
    }
    Ok(out)
}

/// Money that left the business account more than 14 days ago with nothing
/// to show for it: the "bonnetje ontbreekt" list.
pub fn missing_receipts(conn: &Connection, today: NaiveDate) -> ApiResult<Vec<serde_json::Value>> {
    let cutoff = (today - chrono::Duration::days(14)).to_string();
    let rows = crate::entities::query_json(
        conn,
        "SELECT * FROM bank_transactions WHERE amount_cents < 0 AND matched_id = '' AND ignored = 0 AND date <= ?1 ORDER BY date DESC",
        &[&cutoff],
        &["ignored"],
    )?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{create, CLIENTS, DOCUMENTS, INVOICES};
    use serde_json::json;

    #[test]
    fn amounts_from_every_bank() {
        assert_eq!(parse_amount("-12,34"), Some(-1234));
        assert_eq!(parse_amount("+1.234,56"), Some(123456));
        assert_eq!(parse_amount("12.34"), Some(1234));
        assert_eq!(parse_amount("−5,00"), Some(-500));
        assert_eq!(parse_amount("1,234.50"), Some(123450));
        assert_eq!(parse_amount("7"), Some(700));
    }

    #[test]
    fn ing_csv() {
        let csv = "\"Datum\";\"Naam / Omschrijving\";\"Rekening\";\"Tegenrekening\";\"Code\";\"Af Bij\";\"Bedrag (EUR)\";\"Mutatiesoort\";\"Mededelingen\"\n\
                   \"20261003\";\"Hetzner Online\";\"NL01INGB0001234567\";\"DE02100100100006820101\";\"IC\";\"Af\";\"4,51\";\"Incasso\";\"Invoice 081234\"\n\
                   \"20261004\";\"Klant BV\";\"NL01INGB0001234567\";\"NL91ABNA0417164300\";\"OV\";\"Bij\";\"1512,50\";\"Overschrijving\";\"Factuur 2026-001\"";
        let t = parse_csv(csv).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], Tx { date: "2026-10-03".into(), amount_cents: -451, counterparty: "Hetzner Online".into(), iban: "DE02100100100006820101".into(), description: "Invoice 081234".into() });
        assert_eq!(t[1].amount_cents, 151250);
    }

    #[test]
    fn rabobank_csv() {
        let csv = "\"IBAN/BBAN\",\"Munt\",\"BIC\",\"Volgnr\",\"Datum\",\"Rentedatum\",\"Bedrag\",\"Saldo na trn\",\"Tegenrekening IBAN/BBAN\",\"Naam tegenpartij\",\"Omschrijving-1\",\"Omschrijving-2\"\n\
                   \"NL11RABO0123456789\",\"EUR\",\"RABONL2U\",\"1\",\"2026-10-03\",\"2026-10-03\",\"-12,10\",\"100,00\",\"\",\"Q-Park\",\"Parkeren\",\"Hilversum\"";
        let t = parse_csv(csv).unwrap();
        assert_eq!(t[0].amount_cents, -1210);
        assert_eq!(t[0].counterparty, "Q-Park");
        assert_eq!(t[0].description, "Parkeren Hilversum");
    }

    #[test]
    fn camt() {
        let xml = r#"<Document><BkToCstmrStmt><Stmt><Ntry><Amt Ccy="EUR">4.51</Amt><CdtDbtInd>DBIT</CdtDbtInd><BookgDt><Dt>2026-10-03</Dt></BookgDt>
            <NtryDtls><TxDtls><RltdPties><Cdtr><Nm>Hetzner Online GmbH</Nm></Cdtr><CdtrAcct><Id><IBAN>DE02100100100006820101</IBAN></Id></CdtrAcct></RltdPties>
            <RmtInf><Ustrd>Invoice 081234</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry></Stmt></BkToCstmrStmt></Document>"#;
        let t = parse_camt(xml).unwrap();
        assert_eq!(t[0], Tx { date: "2026-10-03".into(), amount_cents: -451, counterparty: "Hetzner Online GmbH".into(), iban: "DE02100100100006820101".into(), description: "Invoice 081234".into() });
    }

    #[test]
    fn import_dedupes_matches_invoices_and_suggests_documents() {
        let pool = db::memory();
        let mut c = pool.get().unwrap();
        create(&mut c, &CLIENTS, &json!({"id": "c", "name": "Klant"})).unwrap();
        create(&mut c, &INVOICES, &json!({"id": "i", "client_id": "c", "date": "2026-10-01",
            "lines": [{"quantity_milli": 1000, "unit_cents": 125000, "vat_rate": "21"}]})).unwrap();
        crate::invoices::finalize(&mut c, "i").unwrap();
        create(&mut c, &DOCUMENTS, &json!({"id": "d", "date": "2026-10-02", "supplier_name": "Hetzner Online", "total_cents": 451})).unwrap();
        let txs = vec![
            Tx { date: "2026-10-04".into(), amount_cents: 151250, counterparty: "Klant BV".into(), iban: "".into(), description: "Factuur 2026-001".into() },
            Tx { date: "2026-10-03".into(), amount_cents: -451, counterparty: "Hetzner Online".into(), iban: "".into(), description: "".into() },
            Tx { date: "2026-09-01".into(), amount_cents: -999, counterparty: "Onbekend".into(), iban: "".into(), description: "".into() },
        ];
        let r = import(&mut c, &txs).unwrap();
        assert_eq!((r.added, r.duplicates, r.matched), (3, 0, 1));
        assert_eq!(import(&mut c, &txs).unwrap().duplicates, 3);
        let paid: String = c.query_row("SELECT status FROM invoices WHERE id = 'i'", [], |r| r.get(0)).unwrap();
        assert_eq!(paid, "betaald");
        let s = suggestions(&c).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].kind.as_str(), s[0].id.as_str()), ("document", "d"));
        let missing = missing_receipts(&c, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()).unwrap();
        assert_eq!(missing.len(), 1);
    }
}
