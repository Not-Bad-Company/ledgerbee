//! The SQLite database: pool, schema, small helpers.

use anyhow::Result;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

pub type Pool = r2d2::Pool<SqliteConnectionManager>;

/// Each entry is one migration; `PRAGMA user_version` counts how many ran.
/// Append, never edit.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    totp_sealed TEXT NOT NULL,
    totp_last_step INTEGER NOT NULL DEFAULT 0,
    recovery_hashes TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);
CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    last_seen TEXT NOT NULL
);
CREATE TABLE login_attempts (at TEXT NOT NULL, ok INTEGER NOT NULL);

CREATE TABLE categories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    ib_group TEXT NOT NULL DEFAULT 'overig',
    deductible_pct INTEGER NOT NULL DEFAULT 100,
    vat_deductible INTEGER NOT NULL DEFAULT 1,
    archived INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE suppliers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kvk TEXT NOT NULL DEFAULT '',
    vat_id TEXT NOT NULL DEFAULT '',
    default_category_id TEXT,
    default_vat_rate TEXT NOT NULL DEFAULT '',
    match_terms TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'bonnetje',
    date TEXT NOT NULL,
    supplier_name TEXT NOT NULL DEFAULT '',
    supplier_id TEXT,
    total_cents INTEGER NOT NULL DEFAULT 0,
    file_sha TEXT NOT NULL DEFAULT '',
    file_type TEXT NOT NULL DEFAULT '',
    ocr_text TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'concept',
    note TEXT NOT NULL DEFAULT '',
    paid_private INTEGER NOT NULL DEFAULT 0,
    parse_notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE document_lines (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    description TEXT NOT NULL DEFAULT '',
    amount_excl_cents INTEGER NOT NULL DEFAULT 0,
    vat_rate TEXT NOT NULL DEFAULT '21',
    vat_cents INTEGER NOT NULL DEFAULT 0,
    category_id TEXT,
    business_pct INTEGER NOT NULL DEFAULT 100,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE clients (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    contact TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    street TEXT NOT NULL DEFAULT '',
    postcode TEXT NOT NULL DEFAULT '',
    city TEXT NOT NULL DEFAULT '',
    country TEXT NOT NULL DEFAULT 'NL',
    kvk TEXT NOT NULL DEFAULT '',
    vat_id TEXT NOT NULL DEFAULT '',
    is_business INTEGER NOT NULL DEFAULT 1,
    payment_days INTEGER NOT NULL DEFAULT 14,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE invoices (
    id TEXT PRIMARY KEY,
    number TEXT,
    client_id TEXT REFERENCES clients(id),
    date TEXT NOT NULL,
    due_date TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'concept',
    vat_scheme TEXT NOT NULL DEFAULT 'nl',
    note TEXT NOT NULL DEFAULT '',
    sent_at TEXT,
    paid_at TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX invoices_number ON invoices(number) WHERE number IS NOT NULL;
CREATE TABLE invoice_lines (
    id TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    description TEXT NOT NULL DEFAULT '',
    quantity_milli INTEGER NOT NULL DEFAULT 1000,
    unit_cents INTEGER NOT NULL DEFAULT 0,
    vat_rate TEXT NOT NULL DEFAULT '21',
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE bank_transactions (
    id TEXT PRIMARY KEY,
    date TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    counterparty TEXT NOT NULL DEFAULT '',
    iban TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    import_hash TEXT NOT NULL UNIQUE,
    matched_kind TEXT NOT NULL DEFAULT '',
    matched_id TEXT NOT NULL DEFAULT '',
    ignored INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE assets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    purchase_date TEXT NOT NULL,
    cost_cents INTEGER NOT NULL,
    useful_years INTEGER NOT NULL DEFAULT 5,
    residual_cents INTEGER NOT NULL DEFAULT 0,
    business_pct INTEGER NOT NULL DEFAULT 100,
    document_id TEXT,
    disposed_date TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE hours (
    id TEXT PRIMARY KEY,
    date TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    client_id TEXT,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE trips (
    id TEXT PRIMARY KEY,
    date TEXT NOT NULL,
    origin TEXT NOT NULL DEFAULT '',
    destination TEXT NOT NULL DEFAULT '',
    km_tenths INTEGER NOT NULL,
    return_trip INTEGER NOT NULL DEFAULT 0,
    purpose TEXT NOT NULL DEFAULT '',
    client_id TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE periods (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    filed_at TEXT,
    filed_numbers TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (kind, key)
);
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    tbl TEXT NOT NULL,
    row_id TEXT NOT NULL,
    action TEXT NOT NULL,
    before TEXT,
    after TEXT
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE INDEX documents_date ON documents(date);
CREATE INDEX invoices_date ON invoices(date);
CREATE INDEX bank_date ON bank_transactions(date);
CREATE INDEX hours_date ON hours(date);
CREATE INDEX trips_date ON trips(date);
"#, r#"
-- Payment reminders for overdue invoices.
ALTER TABLE invoices ADD COLUMN reminded_at TEXT;
ALTER TABLE invoices ADD COLUMN reminders INTEGER NOT NULL DEFAULT 0;
"#];

/// Categories every administration starts with. `ib_group` is the cost group
/// of the IB aangifte; `investering` lines become assets, not costs.
const SEED_CATEGORIES: &[(&str, &str, i64, bool)] = &[
    ("Software en abonnementen", "kantoor", 100, true),
    ("Hosting en domeinen", "kantoor", 100, true),
    ("Hardware (< €450)", "kantoor", 100, true),
    ("Investering (≥ €450)", "investering", 100, true),
    ("Kantoorartikelen", "kantoor", 100, true),
    ("Telefoon en internet", "kantoor", 100, true),
    ("Reclame en marketing", "verkoop", 100, true),
    ("Representatie en etentjes", "verkoop", 80, false),
    ("Reiskosten (OV, parkeren)", "auto", 100, true),
    ("Opleiding en boeken", "overig", 100, true),
    ("Bankkosten", "overig", 100, false),
    ("Verzekeringen", "overig", 100, false),
    ("Advies en administratie", "overig", 100, true),
    ("Werkruimte", "huisvesting", 100, true),
    ("Overig", "overig", 100, true),
];

pub fn open(path: &std::path::Path) -> Result<Pool> {
    let manager = SqliteConnectionManager::file(path).with_init(|c| {
        c.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
    });
    let pool = r2d2::Pool::builder().max_size(8).build(manager)?;
    let mut conn = pool.get()?;
    migrate(&mut conn)?;
    drop(conn);
    Ok(pool)
}

/// An in-memory database for tests (one connection, so every handle sees it).
#[cfg(test)]
pub fn memory() -> Pool {
    let manager = SqliteConnectionManager::memory().with_init(|c| c.execute_batch("PRAGMA foreign_keys = ON;"));
    let pool = r2d2::Pool::builder().max_size(1).build(manager).unwrap();
    migrate(&mut *pool.get().unwrap()).unwrap();
    pool
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        if i == 0 {
            let now = now();
            for (name, group, pct, vat) in SEED_CATEGORIES {
                tx.execute(
                    "INSERT INTO categories (id, name, ib_group, deductible_pct, vat_deductible, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    rusqlite::params![new_id(), name, group, pct, vat, now],
                )?;
            }
        }
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Today in the Netherlands.
pub fn today() -> chrono::NaiveDate {
    chrono::Utc::now().with_timezone(&chrono_tz::Europe::Amsterdam).date_naive()
}

pub fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}
