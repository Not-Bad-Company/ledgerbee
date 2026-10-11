//! The JSON API the app talks to. Everything but `/api/login` and
//! `/api/health` needs a device token (`auth::Device`).

use crate::auth::{self, Device};
use crate::db::{self, Pool};
use crate::entities::{self, ListQuery, BANK, DOCUMENTS, INVOICES};
use crate::error::{ApiError, ApiResult};
use crate::{bank, config, export, files, invoices, parse, pdf, reminders, tax};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

/// Runs `f` with a connection on the blocking pool.
async fn with_db<T: Send + 'static>(state: &AppState, f: impl FnOnce(&mut Connection) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    let pool = state.pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        f(&mut conn)
    })
    .await?
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/login", post(login))
        .route("/api/me", get(me))
        .route("/api/devices/:id", delete(delete_device))
        .route("/api/data/:entity", get(list).post(create))
        .route("/api/data/:entity/:id", get(read).put(update).delete(remove))
        .route("/api/parse", post(parse_text))
        .route("/api/files", post(upload))
        .route("/api/files/:sha", get(download))
        .route("/api/invoices/:id/finalize", post(finalize))
        .route("/api/invoices/:id/paid", post(paid))
        .route("/api/invoices/:id/pdf", get(invoice_pdf))
        .route("/api/invoices/:id/send", post(send_invoice))
        .route("/api/invoices/:id/remind", post(remind_invoice))
        .route("/api/bank/import", post(bank_import))
        .route("/api/bank/suggestions", get(bank_suggestions))
        .route("/api/bank/missing", get(bank_missing))
        .route("/api/bank/:id/link", post(bank_link))
        .route("/api/reports/btw/:key", get(btw_report))
        .route("/api/reports/ib/:year", get(ib_report))
        .route("/api/periods/:kind/:key/file", post(file_period))
        .route("/api/periods/:kind/:key/reopen", post(reopen_period))
        .route("/api/overview", get(overview))
        .route("/api/deadlines", get(deadlines))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/export/:year", get(export_year))
        .route("/api/audit", get(audit))
        .layer(DefaultBodyLimit::max(files::MAX_BYTES + 1024 * 1024))
        .with_state(state)
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
    code: String,
    #[serde(default)]
    device: String,
}

async fn login(State(s): State<AppState>, Json(b): Json<LoginBody>) -> ApiResult<Json<Value>> {
    let token = with_db(&s, move |c| auth::login(c, &b.email, &b.password, &b.code, if b.device.is_empty() { "apparaat" } else { &b.device })).await?;
    Ok(Json(json!({ "token": token })))
}

async fn me(State(s): State<AppState>, d: Device) -> ApiResult<Json<Value>> {
    let (devices, company, owner_email) = with_db(&s, |c| {
        let devices = entities::query_json(c, "SELECT id, name, created_at, last_seen FROM devices ORDER BY last_seen DESC", &[], &[])?;
        Ok((devices, config::company(c), config::owner_email(c)))
    })
    .await?;
    Ok(Json(json!({
        "device_id": d.id,
        "company": company,
        "mail_configured": crate::mail::configured(),
        "owner_email": owner_email,
        "devices": devices,
    })))
}

async fn delete_device(State(s): State<AppState>, _d: Device, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| Ok(c.execute("DELETE FROM devices WHERE id = ?1", [id])?)).await?;
    Ok(Json(json!({"ok": true})))
}

fn entity(name: &str) -> ApiResult<&'static entities::Entity> {
    entities::by_name(name).ok_or_else(ApiError::not_found)
}

async fn list(State(s): State<AppState>, _d: Device, Path(name): Path<String>, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let e = entity(&name)?;
    let rows = with_db(&s, move |c| entities::list(c, e, &q)).await?;
    Ok(Json(Value::Array(rows)))
}

async fn read(State(s): State<AppState>, _d: Device, Path((name, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let e = entity(&name)?;
    with_db(&s, move |c| entities::get(c, e, &id)?.ok_or_else(ApiError::not_found)).await.map(Json)
}

/// A document arriving with OCR text and no lines (a phone scan, maybe made
/// offline) is filled in from the text; what the user already typed wins.
fn fill_from_text(c: &Connection, body: &mut Value) {
    let text = body["ocr_text"].as_str().unwrap_or("").to_string();
    let has_lines = body["lines"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    if text.trim().is_empty() || has_lines {
        return;
    }
    let d = parse::parse(&text, db::today(), Some(c));
    let empty = |v: &Value| v.is_null() || v.as_str() == Some("") || v.as_i64() == Some(0);
    if empty(&body["date"]) {
        body["date"] = json!(d.date.clone().unwrap_or_else(|| db::today().to_string()));
    }
    if empty(&body["total_cents"]) {
        body["total_cents"] = json!(d.total_cents.unwrap_or(0));
    }
    if empty(&body["supplier_name"]) {
        body["supplier_name"] = json!(d.supplier_name);
    }
    if body["supplier_id"].is_null() {
        body["supplier_id"] = json!(d.supplier_id);
    }
    body["parse_notes"] = json!(d.notes.join("\n"));
    body["lines"] = json!(d
        .lines
        .iter()
        .map(|l| json!({"amount_excl_cents": l.amount_excl_cents, "vat_rate": l.vat_rate, "vat_cents": l.vat_cents, "category_id": d.category_id}))
        .collect::<Vec<_>>());
}

async fn create(State(s): State<AppState>, _d: Device, Path(name): Path<String>, Json(mut body): Json<Value>) -> ApiResult<Json<Value>> {
    let e = entity(&name)?;
    if e.name == BANK.name {
        return Err(ApiError::bad("Banktransacties komen uit een import"));
    }
    with_db(&s, move |c| {
        if e.name == DOCUMENTS.name {
            fill_from_text(c, &mut body);
            if body["date"].is_null() {
                body["date"] = json!(db::today().to_string());
            }
        }
        if e.name == INVOICES.name {
            body["status"] = json!("concept");
        }
        let row = entities::create(c, e, &body)?;
        if e.name == DOCUMENTS.name && row["status"] == "bevestigd" {
            parse::learn(c, &row)?;
        }
        Ok(row)
    })
    .await
    .map(Json)
}

async fn update(State(s): State<AppState>, _d: Device, Path((name, id)): Path<(String, String)>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let e = entity(&name)?;
    with_db(&s, move |c| {
        if e.name == INVOICES.name {
            invoices::check_update(c, &id, &body)?;
        }
        let row = entities::update(c, e, &id, &body)?;
        if e.name == DOCUMENTS.name && row["status"] == "bevestigd" && body.get("status").is_some() {
            parse::learn(c, &row)?;
        }
        Ok(row)
    })
    .await
    .map(Json)
}

async fn remove(State(s): State<AppState>, _d: Device, Path((name, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let e = entity(&name)?;
    with_db(&s, move |c| {
        if e.name == INVOICES.name {
            invoices::check_delete(c, &id)?;
        }
        if e.name == BANK.name {
            return Err(ApiError::bad("Een banktransactie blijft; markeer hem als privé"));
        }
        entities::delete(c, e, &id)
    })
    .await?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct TextBody {
    text: String,
}

async fn parse_text(State(s): State<AppState>, _d: Device, Json(b): Json<TextBody>) -> ApiResult<Json<Value>> {
    let d = with_db(&s, move |c| Ok(parse::parse(&b.text, db::today(), Some(c)))).await?;
    Ok(Json(json!(d)))
}

/// Stores a file. A PDF also comes back read: its text and a draft.
async fn upload(State(s): State<AppState>, _d: Device, body: Bytes) -> ApiResult<Json<Value>> {
    let kind = files::sniff(&body);
    let bytes = body.to_vec();
    let sha = tokio::task::spawn_blocking({
        let bytes = bytes.clone();
        move || files::store(&bytes)
    })
    .await??;
    let mut out = json!({"sha": sha, "type": kind});
    if kind == "application/pdf" {
        let text = files::pdf_text(&bytes).await.unwrap_or_default();
        if !text.trim().is_empty() {
            let t = text.clone();
            let draft = with_db(&s, move |c| Ok(parse::parse(&t, db::today(), Some(c)))).await?;
            out["text"] = json!(text);
            out["draft"] = json!(draft);
        }
    }
    Ok(Json(out))
}

async fn download(_d: Device, Path(sha): Path<String>) -> ApiResult<Response> {
    let p = files::path(&sha)?;
    let bytes = tokio::fs::read(p).await.map_err(|_| ApiError::not_found())?;
    let kind = files::sniff(&bytes);
    Ok(([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], bytes).into_response())
}

async fn finalize(State(s): State<AppState>, _d: Device, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| {
        let inv = entities::get(c, &INVOICES, &id)?.ok_or_else(ApiError::not_found)?;
        if let Some(why) = entities::lock_for(c, &INVOICES, inv["date"].as_str().unwrap_or(""))? {
            return Err(ApiError::locked(why));
        }
        invoices::finalize(c, &id)
    })
    .await
    .map(Json)
}

#[derive(Deserialize)]
struct PaidBody {
    date: Option<String>,
}

async fn paid(State(s): State<AppState>, _d: Device, Path(id): Path<String>, Json(b): Json<PaidBody>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| invoices::set_paid(c, &id, b.date.as_deref())).await.map(Json)
}

fn invoice_with_client(c: &Connection, id: &str) -> ApiResult<(Value, Value)> {
    let inv = entities::get(c, &INVOICES, id)?.ok_or_else(ApiError::not_found)?;
    let client = inv["client_id"]
        .as_str()
        .map(|cid| entities::get(c, &entities::CLIENTS, cid))
        .transpose()?
        .flatten()
        .unwrap_or(json!({}));
    Ok((inv, client))
}

pub fn invoice_file_name(inv: &Value) -> String {
    format!("Factuur {}.pdf", inv["number"].as_str().unwrap_or("concept"))
}

async fn invoice_pdf(State(s): State<AppState>, _d: Device, Path(id): Path<String>) -> ApiResult<Response> {
    let (inv, client, company) = with_db(&s, move |c| invoice_with_client(c, &id).map(|(i, cl)| (i, cl, config::company(c)))).await?;
    let bytes = pdf::render(&company, &inv, &client)?;
    let disposition = format!("inline; filename=\"{}\"", invoice_file_name(&inv));
    Ok(([(header::CONTENT_TYPE, "application/pdf".to_string()), (header::CONTENT_DISPOSITION, disposition)], bytes).into_response())
}

/// E-mails a final invoice to the client, with a copy to the owner.
async fn send_invoice(State(s): State<AppState>, _d: Device, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    mail_invoice(s, id, false).await
}

/// A friendly payment reminder for an invoice past its due date, the PDF
/// attached again.
async fn remind_invoice(State(s): State<AppState>, _d: Device, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    mail_invoice(s, id, true).await
}

async fn mail_invoice(s: AppState, id: String, reminder: bool) -> ApiResult<Json<Value>> {
    let iid = id.clone();
    let (inv, client, company, owner) =
        with_db(&s, move |c| invoice_with_client(c, &iid).map(|(i, cl)| (i, cl, config::company(c), config::owner_email(c)))).await?;
    if inv["status"] == "concept" {
        return Err(ApiError::bad("Maak de factuur eerst definitief"));
    }
    if reminder && inv["status"] == "betaald" {
        return Err(ApiError::bad("Deze factuur is al betaald"));
    }
    let to = client["email"].as_str().unwrap_or("").to_string();
    if to.is_empty() {
        return Err(ApiError::bad("Deze klant heeft geen e-mailadres"));
    }
    if !crate::mail::configured() {
        return Err(ApiError::bad("E-mail is niet ingesteld op de server (BREVO_API_KEY, EMAIL_FROM_ADDRESS)"));
    }
    let bytes = pdf::render(&company, &inv, &client)?;
    let total = invoices::totals_of(&inv).incl_cents;
    let number = inv["number"].as_str().unwrap_or("").to_string();
    let greeting = crate::mail::escape(client["contact"].as_str().filter(|c| !c.is_empty()).or(client["name"].as_str()).unwrap_or(""));
    let due = pdf::nl_date(inv["due_date"].as_str().unwrap_or(""));
    let pay_to = if company.iban.is_empty() { String::new() } else { format!(" naar {}", crate::mail::escape(&company.iban)) };
    let body = if reminder {
        format!(
            "<p>Beste {greeting},</p><p>Volgens mijn administratie staat factuur {} van {} nog open: {}, met als vervaldatum {due}. \
             Wil je het bedrag{pay_to} overmaken onder vermelding van {}? Is het al betaald, dan kun je dit bericht als niet verzonden beschouwen.</p>\
             <p>De factuur zit er nog een keer bij.</p>",
            crate::mail::escape(&number),
            crate::mail::escape(&company.name),
            pdf::eur(total),
            crate::mail::escape(&number),
        )
    } else {
        format!(
            "<p>Beste {greeting},</p><p>Hierbij ontvang je factuur {} van {} voor een bedrag van {}, te betalen vóór {due}.</p>",
            crate::mail::escape(&number),
            crate::mail::escape(&company.name),
            pdf::eur(total),
        )
    };
    let html = format!("{body}<p>Met vriendelijke groet,<br>{}<br>{}</p>", crate::mail::escape(&company.owner), crate::mail::escape(&company.name));
    let subject = if reminder { format!("Herinnering: factuur {} van {}", number, company.name) } else { format!("Factuur {} van {}", number, company.name) };
    crate::mail::send(&[&to, &owner], &subject, &html, Some(crate::mail::Attachment { name: invoice_file_name(&inv), bytes })).await?;
    with_db(&s, move |c| {
        if reminder {
            c.execute("UPDATE invoices SET reminded_at = ?1, reminders = reminders + 1, updated_at = ?1 WHERE id = ?2", [db::now(), id.clone()])?;
        } else {
            c.execute("UPDATE invoices SET sent_at = ?1, updated_at = ?1 WHERE id = ?2", [db::now(), id.clone()])?;
        }
        entities::audit(c, "invoices", &id, if reminder { "reminded" } else { "sent" }, None, Some(&json!({"to": to})))?;
        Ok(())
    })
    .await?;
    Ok(Json(json!({"ok": true})))
}

async fn bank_import(State(s): State<AppState>, _d: Device, body: Bytes) -> ApiResult<Json<Value>> {
    let bytes = body.to_vec();
    let res = with_db(&s, move |c| {
        files::store(&bytes)?;
        let txs = bank::parse_any(&bytes)?;
        if txs.is_empty() {
            return Err(ApiError::bad("Geen transacties gevonden in dit bestand"));
        }
        bank::import(c, &txs)
    })
    .await?;
    Ok(Json(json!(res)))
}

async fn bank_suggestions(State(s): State<AppState>, _d: Device) -> ApiResult<Json<Value>> {
    with_db(&s, |c| Ok(json!(bank::suggestions(c)?))).await.map(Json)
}

async fn bank_missing(State(s): State<AppState>, _d: Device) -> ApiResult<Json<Value>> {
    with_db(&s, |c| Ok(json!(bank::missing_receipts(c, db::today())?))).await.map(Json)
}

#[derive(Deserialize)]
struct LinkBody {
    kind: String,
    id: String,
}

async fn bank_link(State(s): State<AppState>, _d: Device, Path(tid): Path<String>, Json(b): Json<LinkBody>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| {
        if !matches!(b.kind.as_str(), "invoice" | "document" | "") {
            return Err(ApiError::bad("kind is invoice of document"));
        }
        bank::link(c, &tid, &b.kind, &b.id)?;
        if b.kind == "invoice" {
            let date: String = c.query_row("SELECT date FROM bank_transactions WHERE id = ?1", [&tid], |r| r.get(0))?;
            invoices::set_paid(c, &b.id, Some(&date))?;
        }
        Ok(entities::get(c, &BANK, &tid)?.ok_or_else(ApiError::not_found)?)
    })
    .await
    .map(Json)
}

async fn btw_report(State(s): State<AppState>, _d: Device, Path(key): Path<String>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| Ok(json!(tax::btw::report(c, &key)?))).await.map(Json)
}

async fn ib_report(State(s): State<AppState>, _d: Device, Path(year): Path<i32>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| Ok(json!(tax::ib::report(c, year)?))).await.map(Json)
}

/// Marks a period as filed, keeping a copy of the numbers as they were.
async fn file_period(State(s): State<AppState>, _d: Device, Path((kind, key)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| {
        let snapshot = match kind.as_str() {
            "btw_q" => json!(tax::btw::report(c, &key)?),
            "ib_year" => json!(tax::ib::report(c, key.parse().map_err(|_| ApiError::bad("Jaar als 2026"))?)?),
            _ => return Err(ApiError::bad("kind is btw_q of ib_year")),
        };
        c.execute(
            "INSERT INTO periods (kind, key, status, filed_at, filed_numbers) VALUES (?1, ?2, 'ingediend', ?3, ?4)
             ON CONFLICT(kind, key) DO UPDATE SET status = 'ingediend', filed_at = excluded.filed_at, filed_numbers = excluded.filed_numbers",
            rusqlite::params![kind, key, db::now(), snapshot.to_string()],
        )?;
        entities::audit(c, "periods", &format!("{kind}/{key}"), "file", None, Some(&snapshot))?;
        Ok(json!({"ok": true}))
    })
    .await
    .map(Json)
}

/// Opens a filed period again, for a suppletie. Logged like every change.
async fn reopen_period(State(s): State<AppState>, _d: Device, Path((kind, key)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| {
        c.execute("UPDATE periods SET status = 'open' WHERE kind = ?1 AND key = ?2", [&kind, &key])?;
        entities::audit(c, "periods", &format!("{kind}/{key}"), "reopen", None, None)?;
        Ok(json!({"ok": true}))
    })
    .await
    .map(Json)
}

async fn overview(State(s): State<AppState>, _d: Device) -> ApiResult<Json<Value>> {
    with_db(&s, |c| {
        let today = db::today();
        let q = tax::btw::quarter_of(today);
        let btw = tax::btw::report(c, &q)?;
        let ib = tax::ib::report(c, chrono::Datelike::year(&today))?;
        Ok(json!({
            "today": today.to_string(),
            "deadlines": reminders::deadlines(c, today)?,
            "todo": reminders::todo(c, today)?,
            "quarter": {"key": q, "total_cents": btw.total_cents, "warnings": btw.warnings},
            "year": {
                "year": ib.year, "omzet_cents": ib.omzet_cents, "kosten_cents": ib.kosten_cents + ib.afschrijving_cents + ib.km_cents,
                "winst_cents": ib.winst_cents, "uren": ib.uren, "uren_nodig": ib.uren_nodig, "km": ib.km,
            },
            "company": config::company(c),
        }))
    })
    .await
    .map(Json)
}

async fn deadlines(State(s): State<AppState>, _d: Device) -> ApiResult<Json<Value>> {
    with_db(&s, |c| Ok(json!(reminders::deadlines(c, db::today())?))).await.map(Json)
}

const SETTINGS: &[&str] = &[
    "start_date", "startersaftrek",
    "company_name", "company_owner", "company_street", "company_postcode", "company_city",
    "company_kvk", "company_btw", "company_email", "company_phone", "company_iban",
];

async fn get_settings(State(s): State<AppState>, _d: Device) -> ApiResult<Json<Value>> {
    with_db(&s, |c| {
        let mut out = serde_json::Map::new();
        out.insert("start_date".into(), json!(reminders::start_date(c).to_string()));
        out.insert("startersaftrek".into(), json!(db::setting(c, "startersaftrek").unwrap_or_else(|| "ja".into())));
        // What the app has stored; `company` in /api/me is the result with the env filled in.
        for (field, _) in config::COMPANY_FIELDS {
            let key = format!("company_{field}");
            out.insert(key.clone(), json!(db::setting(c, &key).unwrap_or_default()));
        }
        Ok(Value::Object(out))
    })
    .await
    .map(Json)
}

async fn put_settings(State(s): State<AppState>, _d: Device, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    with_db(&s, move |c| {
        for (k, v) in body.as_object().ok_or_else(|| ApiError::bad("Verwacht een object"))? {
            if !SETTINGS.contains(&k.as_str()) {
                return Err(ApiError::bad(format!("Onbekende instelling {k}")));
            }
            let v = v.as_str().ok_or_else(|| ApiError::bad("Tekst verwacht"))?;
            if k == "start_date" && chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
                return Err(ApiError::bad("Datum als 2026-01-31"));
            }
            if k == "company_kvk" && !v.trim().is_empty() && !(v.trim().len() == 8 && v.trim().chars().all(|c| c.is_ascii_digit())) {
                return Err(ApiError::bad("Een KvK-nummer heeft 8 cijfers"));
            }
            db::set_setting(c, k, v)?;
            entities::audit(c, "settings", k, "update", None, Some(&json!(v)))?;
        }
        Ok(json!({"ok": true}))
    })
    .await
    .map(Json)
}

async fn export_year(State(s): State<AppState>, _d: Device, Path(year): Path<i32>) -> ApiResult<Response> {
    let bytes = with_db(&s, move |c| Ok(export::year(c, year)?)).await?;
    let disposition = format!("attachment; filename=\"administratie-{year}.zip\"");
    Ok(([(header::CONTENT_TYPE, "application/zip".to_string()), (header::CONTENT_DISPOSITION, disposition)], bytes).into_response())
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn audit(State(s): State<AppState>, _d: Device, Query(q): Query<AuditQuery>) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2000);
    with_db(&s, move |c| Ok(json!(entities::query_json(c, "SELECT * FROM audit_log ORDER BY id DESC LIMIT ?1", &[&limit], &[])?))).await.map(Json)
}
