use crate::entities::{self, ListQuery, ASSETS, BANK, DOCUMENTS, HOURS, TRIPS};
use crate::{config, files, invoices, pdf, tax};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;
use std::io::Write;
use zip::write::SimpleFileOptions;

fn csv_of(rows: &[Vec<String>]) -> Result<Vec<u8>> {
    let mut w = csv::WriterBuilder::new().delimiter(b';').from_writer(vec![]);
    for r in rows {
        w.write_record(r)?;
    }
    Ok(w.into_inner()?)
}

fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn cents(v: &Value) -> String {
    let c = v.as_i64().unwrap_or(0);
    format!("{}{},{:02}", if c < 0 { "-" } else { "" }, c.abs() / 100, c.abs() % 100)
}

fn ext(kind: &str) -> &'static str {
    match kind {
        "application/pdf" => "pdf",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => "bin",
    }
}

pub fn year(conn: &Connection, year: i32) -> Result<Vec<u8>> {
    let q = ListQuery { from: Some(format!("{year}-01-01")), to: Some(format!("{year}-12-31")), ..Default::default() };
    let err = |e: crate::error::ApiError| anyhow::anyhow!(e.1);
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = SimpleFileOptions::default();
    let cats: std::collections::HashMap<String, String> = {
        let mut st = conn.prepare("SELECT id, name FROM categories")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };

    let docs = entities::list(conn, &DOCUMENTS, &q).map_err(err)?;
    let mut rows = vec![["datum", "soort", "leverancier", "status", "totaal", "omschrijving", "excl. btw", "tarief", "btw", "categorie", "zakelijk %", "bestand"]
        .map(String::from)
        .to_vec()];
    for d in &docs {
        let file = if s(&d["file_sha"]).is_empty() { String::new() } else { format!("bonnen/{}.{}", s(&d["file_sha"]), ext(&s(&d["file_type"]))) };
        for l in d["lines"].as_array().into_iter().flatten() {
            rows.push(vec![
                s(&d["date"]),
                s(&d["kind"]),
                s(&d["supplier_name"]),
                s(&d["status"]),
                cents(&d["total_cents"]),
                s(&l["description"]),
                cents(&l["amount_excl_cents"]),
                s(&l["vat_rate"]),
                cents(&l["vat_cents"]),
                l["category_id"].as_str().and_then(|c| cats.get(c)).cloned().unwrap_or_default(),
                s(&l["business_pct"]),
                file.clone(),
            ]);
        }
        if !s(&d["file_sha"]).is_empty() {
            if let Ok(bytes) = files::path(&s(&d["file_sha"])).and_then(|p| Ok(std::fs::read(p)?)) {
                zip.start_file(&file, opt)?;
                zip.write_all(&bytes)?;
            }
        }
    }
    zip.start_file("kosten.csv", opt)?;
    zip.write_all(&csv_of(&rows)?)?;

    let company = config::company(conn);
    let mut rows = vec![["nummer", "datum", "vervaldatum", "klant", "status", "excl. btw", "btw", "totaal", "betaald op"].map(String::from).to_vec()];
    for inv in invoices::final_between(conn, &q.from.clone().unwrap(), &q.to.clone().unwrap()).map_err(err)? {
        let client = inv["client_id"].as_str().and_then(|c| entities::get(conn, &entities::CLIENTS, c).ok().flatten()).unwrap_or(Value::Null);
        let t = invoices::totals_of(&inv);
        rows.push(vec![
            s(&inv["number"]),
            s(&inv["date"]),
            s(&inv["due_date"]),
            s(&client["name"]),
            s(&inv["status"]),
            cents(&t.excl_cents.into()),
            cents(&t.vat_cents.into()),
            cents(&t.incl_cents.into()),
            s(&inv["paid_at"]),
        ]);
        zip.start_file(format!("facturen/Factuur {}.pdf", s(&inv["number"])), opt)?;
        zip.write_all(&pdf::render(&company, &inv, &client)?)?;
    }
    zip.start_file("facturen.csv", opt)?;
    zip.write_all(&csv_of(&rows)?)?;

    let simple: [(&str, &entities::Entity, &[&str]); 4] = [
        ("bank.csv", &BANK, &["date", "amount_cents", "counterparty", "iban", "description", "matched_kind", "ignored"]),
        ("uren.csv", &HOURS, &["date", "minutes", "client_id", "description"]),
        ("ritten.csv", &TRIPS, &["date", "origin", "destination", "km_tenths", "return_trip", "purpose"]),
        ("activa.csv", &ASSETS, &["name", "purchase_date", "cost_cents", "useful_years", "residual_cents", "business_pct", "disposed_date"]),
    ];
    for (name, e, cols) in simple {
        let list = if e.name == "assets" { entities::list(conn, e, &Default::default()) } else { entities::list(conn, e, &q) }.map_err(err)?;
        let mut rows = vec![cols.iter().map(|c| c.to_string()).collect::<Vec<_>>()];
        for r in list {
            rows.push(cols.iter().map(|c| if c.ends_with("_cents") { cents(&r[*c]) } else { s(&r[*c]) }).collect());
        }
        zip.start_file(name, opt)?;
        zip.write_all(&csv_of(&rows)?)?;
    }

    for qn in 1..=4 {
        let key = format!("{year}-Q{qn}");
        let rep = tax::btw::report(conn, &key).map_err(err)?;
        zip.start_file(format!("btw-{key}.json"), opt)?;
        zip.write_all(serde_json::to_string_pretty(&rep)?.as_bytes())?;
    }
    zip.start_file(format!("inkomstenbelasting-{year}.json"), opt)?;
    zip.write_all(serde_json::to_string_pretty(&tax::ib::report(conn, year).map_err(err)?)?.as_bytes())?;
    Ok(zip.finish()?.into_inner())
}
