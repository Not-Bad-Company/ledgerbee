use crate::db;
use crate::error::ApiResult;
use crate::tax::btw::{quarter_bounds, quarter_of};
use chrono::{Datelike, NaiveDate, Timelike};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Deadline {
    pub kind: String,
    pub key: String,
    pub title: String,
    pub due: String,
    pub filed: bool,
    pub days_left: i64,
}

fn filed(conn: &Connection, kind: &str, key: &str) -> bool {
    conn.query_row("SELECT status = 'ingediend' FROM periods WHERE kind = ?1 AND key = ?2", [kind, key], |r| r.get(0)).unwrap_or(false)
}

pub fn start_date(conn: &Connection) -> NaiveDate {
    let parse = |s: String| NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d").ok();
    db::setting(conn, "start_date")
        .and_then(parse)
        .or_else(|| conn.query_row("SELECT min(created_at) FROM users", [], |r| r.get::<_, Option<String>>(0)).ok().flatten().and_then(parse))
        .unwrap_or_else(db::today)
}

pub fn deadlines(conn: &Connection, today: NaiveDate) -> ApiResult<Vec<Deadline>> {
    let start = start_date(conn);
    let horizon = today + chrono::Duration::days(366);
    let mut out = vec![];
    let mut q = quarter_of(start);
    loop {
        let (_, end, due) = quarter_bounds(&q)?;
        if due > horizon {
            break;
        }
        let f = filed(conn, "btw_q", &q);
        if !f || due >= today {
            out.push(Deadline {
                kind: "btw_q".into(),
                title: format!("Btw-aangifte {}", q.replace('-', " ")),
                key: q.clone(),
                due: due.to_string(),
                filed: f,
                days_left: (due - today).num_days(),
            });
        }
        q = quarter_of(end + chrono::Duration::days(1));
    }
    for year in start.year()..=today.year() {
        let due = NaiveDate::from_ymd_opt(year + 1, 5, 1).expect("date");
        let f = filed(conn, "ib_year", &year.to_string());
        if due <= horizon && (!f || due >= today) {
            out.push(Deadline {
                kind: "ib_year".into(),
                key: year.to_string(),
                title: format!("Aangifte inkomstenbelasting {year}"),
                due: due.to_string(),
                filed: f,
                days_left: (due - today).num_days(),
            });
        }
    }
    out.sort_by(|a, b| a.due.cmp(&b.due));
    Ok(out)
}

#[derive(Serialize, Debug, Default)]
pub struct Todo {
    pub concept_documents: i64,
    pub missing_receipts: i64,
    pub bank_suggestions: i64,
    pub overdue_invoices: Vec<serde_json::Value>,
    pub concept_invoices: i64,
    pub last_bank_import: Option<String>,
}

pub fn todo(conn: &Connection, today: NaiveDate) -> ApiResult<Todo> {
    let count = |sql: &str| -> rusqlite::Result<i64> { conn.query_row(sql, [], |r| r.get(0)) };
    let overdue = crate::entities::query_json(
        conn,
        "SELECT i.id, i.number, i.due_date, c.name AS client FROM invoices i LEFT JOIN clients c ON c.id = i.client_id
         WHERE i.status = 'verzonden' AND i.due_date < ?1 ORDER BY i.due_date",
        &[&today.to_string()],
        &[],
    )?;
    Ok(Todo {
        concept_documents: count("SELECT count(*) FROM documents WHERE status = 'concept'")?,
        missing_receipts: crate::bank::missing_receipts(conn, today)?.len() as i64,
        bank_suggestions: crate::bank::suggestions(conn)?.len() as i64,
        overdue_invoices: overdue,
        concept_invoices: count("SELECT count(*) FROM invoices WHERE status = 'concept'")?,
        last_bank_import: conn.query_row("SELECT max(created_at) FROM bank_transactions", [], |r| r.get(0)).ok().flatten(),
    })
}

pub fn digest(conn: &Connection, today: NaiveDate) -> ApiResult<Option<(String, String)>> {
    let mut items: Vec<String> = vec![];
    let mut subject = None;
    for d in deadlines(conn, today)? {
        if d.filed {
            continue;
        }
        let msg = match d.days_left {
            n if n < 0 => Some(format!("<b>{}</b> was op {} en is nog niet als ingediend gemarkeerd.", d.title, d.due)),
            0 => Some(format!("<b>{}</b>: vandaag is de laatste dag.", d.title)),
            1 | 7 | 14 => Some(format!("<b>{}</b>: nog {} dag(en), uiterlijk {}.", d.title, d.days_left, d.due)),
            30 if d.kind == "ib_year" => Some(format!("<b>{}</b>: nog een maand, uiterlijk {}.", d.title, d.due)),
            _ => None,
        };
        if let Some(m) = msg {
            subject.get_or_insert_with(|| format!("{} — {}", d.title, if d.days_left <= 0 { "nu doen".to_string() } else { format!("nog {} dagen", d.days_left) }));
            items.push(m);
        }
    }
    let t = todo(conn, today)?;
    for inv in &t.overdue_invoices {
        let due = inv["due_date"].as_str().unwrap_or("");
        let days = NaiveDate::parse_from_str(due, "%Y-%m-%d").map(|d| (today - d).num_days()).unwrap_or(0);
        if days == 1 || days % 7 == 0 {
            items.push(format!(
                "Factuur <b>{}</b> aan {} is sinds {} over de vervaldatum.",
                crate::mail::escape(inv["number"].as_str().unwrap_or("")),
                crate::mail::escape(inv["client"].as_str().unwrap_or("")),
                due
            ));
        }
    }
    if today.day() == 1 {
        subject.get_or_insert_with(|| "Maandafsluiting".to_string());
        items.push(format!(
            "Maandafsluiting: importeer het bankafschrift van vorige maand, bevestig {} bonnetje(s) en zoek {} ontbrekende bon(nen).",
            t.concept_documents, t.missing_receipts
        ));
    }
    if items.is_empty() {
        return Ok(None);
    }
    let subject = subject.unwrap_or_else(|| "Boekhouding: openstaande punten".into());
    let html = format!(
        "<p>Goedemorgen,</p><ul>{}</ul><p style=\"color:#666\">Boekhouding {}</p>",
        items.iter().map(|i| format!("<li>{i}</li>")).collect::<String>(),
        crate::mail::escape(&crate::config::company(conn).name),
    );
    Ok(Some((subject, html)))
}

pub async fn run(pool: db::Pool) {
    loop {
        let now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::Amsterdam);
        if now.hour() >= 8 {
            let pool = pool.clone();
            let today = now.date_naive();
            let job = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<(String, String, String)>> {
                let conn = pool.get()?;
                if db::setting(&conn, "last_digest").as_deref() == Some(today.to_string().as_str()) {
                    return Ok(None);
                }
                db::set_setting(&conn, "last_digest", &today.to_string())?;
                let to = crate::config::owner_email(&conn);
                Ok(digest(&conn, today).map_err(|e| anyhow::anyhow!(e.1))?.map(|(s, h)| (s, h, to)))
            })
            .await;
            match job {
                Ok(Ok(Some((subject, html, to)))) => {
                    if !crate::mail::configured() {
                        tracing::warn!("reminder not sent, e-mail is not configured: {subject}");
                    } else if let Err(e) = crate::mail::send(&[&to], &subject, &html, None).await {
                        tracing::error!("reminder e-mail failed: {e:#}");
                    } else {
                        tracing::info!("reminder sent: {subject}");
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(e)) => tracing::error!("reminder job failed: {e:#}"),
                Err(e) => tracing::error!("reminder job panicked: {e}"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn deadlines_from_the_registration() {
        let pool = db::memory();
        let c = pool.get().unwrap();
        let list = deadlines(&c, d("2026-10-06")).unwrap();
        let keys: Vec<(&str, &str)> = list.iter().map(|x| (x.key.as_str(), x.due.as_str())).collect();
        assert_eq!(keys, vec![("2026-Q4", "2027-01-31"), ("2027-Q1", "2027-04-30"), ("2026", "2027-05-01"), ("2027-Q2", "2027-07-31")]);
        assert_eq!(list[0].days_left, 117);
    }

    #[test]
    fn the_digest_speaks_on_reminder_days_only() {
        let pool = db::memory();
        let c = pool.get().unwrap();
        assert!(digest(&c, d("2027-01-10")).unwrap().is_none());
        let (subject, html) = digest(&c, d("2027-01-24")).unwrap().unwrap();
        assert!(subject.starts_with("Btw-aangifte 2026 Q4"), "{subject}");
        assert!(html.contains("nog 7 dag"));
        c.execute("INSERT INTO periods (kind, key, status) VALUES ('btw_q', '2026-Q4', 'ingediend')", []).unwrap();
        assert!(digest(&c, d("2027-01-24")).unwrap().is_none());
        assert!(digest(&c, d("2027-02-01")).unwrap().unwrap().0.contains("Maandafsluiting"));
    }
}
