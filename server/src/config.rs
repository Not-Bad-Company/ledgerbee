//! Settings from the environment.
//!
//! Secrets and paths come from `server/.env` (or the systemd unit's
//! `EnvironmentFile`). The company's public details (name, KvK, btw-id,
//! address) come from the one `.env` above every Not Bad Company project, the
//! same file the websites read, so the invoices and krentenweger.nl cannot
//! disagree about the KvK number.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Loads `./.env`, then the shared company file: `NBC_ENV_FILE`, or the first
/// `.env` that sets `NBC_NAME` walking up from the working directory. Values
/// already in the environment win.
pub fn load() {
    load_file(Path::new(".env"));
    let file = std::env::var("NBC_ENV_FILE").ok().map(PathBuf::from).or_else(|| {
        let mut dir = std::env::current_dir().ok();
        while let Some(d) = dir {
            let candidate = d.join(".env");
            if std::fs::read_to_string(&candidate).map(|s| s.lines().any(|l| l.starts_with("NBC_NAME="))).unwrap_or(false) {
                return Some(candidate);
            }
            dir = d.parent().map(Path::to_path_buf);
        }
        None
    });
    if let Some(file) = file {
        load_file(&file);
    }
}

/// `KEY=value` lines the way systemd's `EnvironmentFile` reads them: spaces
/// inside an unquoted value are kept (`NBC_NAME=Not Bad Company`), which
/// dotenv parsers refuse. `#` lines are comments; one pair of surrounding
/// quotes is removed. A variable already set is left alone.
fn load_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    for (key, value) in parse_env(&text) {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("export ").unwrap_or(l).split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let v = if v.len() >= 2 && (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\'')) { &v[1..v.len() - 1] } else { v };
            (k.trim().to_string(), v.to_string())
        })
        .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn env_files_read_like_systemd() {
        let got = super::parse_env("# c\nNBC_NAME=Not Bad Company\nexport A=\"x y\"\nB='z'\nbad line\n");
        assert_eq!(got, vec![("NBC_NAME".into(), "Not Bad Company".into()), ("A".into(), "x y".into()), ("B".into(), "z".into())]);
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("BOEK_DATA").unwrap_or_else(|_| "data".into()))
}

pub fn bind() -> String {
    std::env::var("BOEK_BIND").unwrap_or_else(|_| "127.0.0.1:8090".into())
}

/// The master secret that seals the TOTP secret. 32+ characters in
/// production; debug builds and tests fall back to a fixed development key.
pub fn secret() -> Vec<u8> {
    static SECRET: OnceLock<Vec<u8>> = OnceLock::new();
    SECRET
        .get_or_init(|| match std::env::var("BOEK_SECRET") {
            Ok(s) if s.len() >= 32 => s.into_bytes(),
            _ if cfg!(debug_assertions) => b"boekhouding-development-secret-not-for-production".to_vec(),
            _ => panic!("BOEK_SECRET must be set to 32+ characters"),
        })
        .clone()
}

fn var(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}

/// A value of X's with no digit in it does not exist yet (the shared .env's
/// placeholder convention).
fn real(v: String) -> String {
    if v.to_ascii_uppercase().contains("XX") && !v.chars().any(|c| c.is_ascii_digit()) {
        String::new()
    } else {
        v
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct Company {
    pub name: String,
    pub owner: String,
    pub street: String,
    pub postcode: String,
    pub city: String,
    pub kvk: String,
    pub btw: String,
    pub email: String,
    pub phone: String,
    pub iban: String,
}

pub fn company() -> Company {
    Company {
        name: var("NBC_NAME"),
        owner: var("NBC_FOUNDER"),
        street: var("NBC_STREET"),
        postcode: var("NBC_POSTCODE"),
        city: var("NBC_CITY"),
        kvk: real(var("NBC_KVK")),
        btw: real(var("NBC_BTW")),
        email: var("NBC_EMAIL"),
        phone: var("NBC_PHONE"),
        // Not public in the shared file on purpose: the server's own .env.
        iban: var("BOEK_IBAN"),
    }
}

/// Where reminders go.
pub fn owner_email() -> String {
    let own = var("BOEK_OWNER_EMAIL");
    if own.is_empty() {
        var("NBC_EMAIL")
    } else {
        own
    }
}

pub fn brevo_key() -> Option<String> {
    Some(var("BREVO_API_KEY")).filter(|k| !k.is_empty())
}
