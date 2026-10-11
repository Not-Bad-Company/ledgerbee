use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

    #[test]
    fn the_app_setting_beats_the_environment() {
        let pool = crate::db::memory();
        let c = pool.get().unwrap();
        std::env::set_var("BOEK_COMPANY_CITY", "Utrecht");
        assert_eq!(super::company(&c).city, "Utrecht");
        crate::db::set_setting(&c, "company_city", "Zwolle").unwrap();
        assert_eq!(super::company(&c).city, "Zwolle");
        crate::db::set_setting(&c, "company_city", " ").unwrap();
        assert_eq!(super::company(&c).city, "Utrecht", "an emptied setting falls back again");
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("BOEK_DATA").unwrap_or_else(|_| "data".into()))
}

pub fn bind() -> String {
    std::env::var("BOEK_BIND").unwrap_or_else(|_| "127.0.0.1:8090".into())
}

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

fn real(v: String) -> String {
    if v.to_ascii_uppercase().contains("XX") && !v.chars().any(|c| c.is_ascii_digit()) {
        String::new()
    } else {
        v
    }
}

pub const COMPANY_FIELDS: &[(&str, &[&str])] = &[
    ("name", &["BOEK_COMPANY_NAME", "NBC_NAME"]),
    ("owner", &["BOEK_COMPANY_OWNER", "NBC_FOUNDER"]),
    ("street", &["BOEK_COMPANY_STREET", "NBC_STREET"]),
    ("postcode", &["BOEK_COMPANY_POSTCODE", "NBC_POSTCODE"]),
    ("city", &["BOEK_COMPANY_CITY", "NBC_CITY"]),
    ("kvk", &["BOEK_COMPANY_KVK", "NBC_KVK"]),
    ("btw", &["BOEK_COMPANY_BTW", "NBC_BTW"]),
    ("email", &["BOEK_COMPANY_EMAIL", "NBC_EMAIL"]),
    ("phone", &["BOEK_COMPANY_PHONE", "NBC_PHONE"]),
    ("iban", &["BOEK_IBAN"]),
];

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

pub fn company(conn: &rusqlite::Connection) -> Company {
    let get = |field: &str| -> String {
        let (_, envs) = COMPANY_FIELDS.iter().find(|(f, _)| *f == field).expect("company field");
        let set = crate::db::setting(conn, &format!("company_{field}")).map(|v| v.trim().to_string()).unwrap_or_default();
        if !set.is_empty() {
            return set;
        }
        envs.iter().map(|k| real(var(k))).find(|v| !v.is_empty()).unwrap_or_default()
    };
    Company {
        name: get("name"),
        owner: get("owner"),
        street: get("street"),
        postcode: get("postcode"),
        city: get("city"),
        kvk: get("kvk"),
        btw: get("btw"),
        email: get("email"),
        phone: get("phone"),
        iban: get("iban"),
    }
}

pub fn owner_email(conn: &rusqlite::Connection) -> String {
    let own = var("BOEK_OWNER_EMAIL");
    if own.is_empty() {
        company(conn).email
    } else {
        own
    }
}

pub fn brevo_key() -> Option<String> {
    Some(var("BREVO_API_KEY")).filter(|k| !k.is_empty())
}
