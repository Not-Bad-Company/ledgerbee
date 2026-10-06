//! One account: e-mail, password (argon2) and TOTP. A login hands the device
//! a random token; only its SHA-256 is stored, and each device can be
//! signed out from the others.

use crate::api::AppState;
use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::totp;
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rand::RngCore;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub struct Enrolment {
    pub otpauth_uri: String,
    pub qr_rows: Vec<String>,
    pub recovery_codes: Vec<String>,
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Creates the account, or replaces it (a new password and a new TOTP
/// secret, and every device signed out).
pub fn create_user(conn: &Connection, email: &str, password: &str) -> anyhow::Result<Enrolment> {
    if password.len() < 12 {
        anyhow::bail!("password must be at least 12 characters");
    }
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string();
    let secret = totp::new_secret();
    let codes = totp::new_recovery_codes();
    let hashes: Vec<String> = codes.iter().map(|c| totp::hash_recovery(c)).collect();
    conn.execute("DELETE FROM users", [])?;
    conn.execute("DELETE FROM devices", [])?;
    conn.execute(
        "INSERT INTO users (id, email, password_hash, totp_sealed, recovery_hashes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![db::new_id(), email.trim().to_lowercase(), hash, totp::seal(&secret), serde_json::to_string(&hashes)?, db::now()],
    )?;
    let uri = totp::otpauth_uri(&secret, email);
    Ok(Enrolment { qr_rows: totp::qr_rows(&uri), otpauth_uri: uri, recovery_codes: codes })
}

/// Checks e-mail, password and code (or a recovery code), and returns a new
/// device token. Ten failures in fifteen minutes close the door for the rest
/// of that window.
pub fn login(conn: &Connection, email: &str, password: &str, code: &str, device: &str) -> ApiResult<String> {
    let since = (chrono::Utc::now() - chrono::Duration::minutes(15)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let failures: i64 = conn.query_row("SELECT count(*) FROM login_attempts WHERE ok = 0 AND at >= ?1", [&since], |r| r.get(0))?;
    if failures >= 10 {
        return Err(ApiError(axum::http::StatusCode::TOO_MANY_REQUESTS, "Te veel pogingen. Probeer het over een kwartier opnieuw.".into()));
    }
    let fail = |conn: &Connection| -> ApiResult<String> {
        conn.execute("INSERT INTO login_attempts (at, ok) VALUES (?1, 0)", [db::now()])?;
        Err(ApiError(axum::http::StatusCode::UNAUTHORIZED, "E-mail, wachtwoord of code klopt niet".into()))
    };
    let user: Option<(String, String, String, i64, String)> = conn
        .query_row(
            "SELECT id, password_hash, totp_sealed, totp_last_step, recovery_hashes FROM users WHERE email = ?1",
            [email.trim().to_lowercase()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok();
    let Some((uid, phash, sealed, last_step, recovery)) = user else { return fail(conn) };
    let Ok(parsed) = PasswordHash::new(&phash) else { return fail(conn) };
    if Argon2::default().verify_password(password.as_bytes(), &parsed).is_err() {
        return fail(conn);
    }
    let secret = totp::open(&sealed).ok_or_else(|| ApiError::bad("Het TOTP-geheim is niet te openen; zet BOEK_SECRET terug of maak het account opnieuw"))?;
    match totp::matching_step(&secret, code, chrono::Utc::now().timestamp()) {
        Some(step) if step > last_step => {
            conn.execute("UPDATE users SET totp_last_step = ?1 WHERE id = ?2", rusqlite::params![step, uid])?;
        }
        _ => {
            let mut hashes: Vec<String> = serde_json::from_str(&recovery).unwrap_or_default();
            let h = totp::hash_recovery(code);
            let Some(pos) = hashes.iter().position(|x| *x == h) else { return fail(conn) };
            hashes.remove(pos);
            conn.execute("UPDATE users SET recovery_hashes = ?1 WHERE id = ?2", rusqlite::params![serde_json::to_string(&hashes).unwrap_or_default(), uid])?;
        }
    }
    conn.execute("INSERT INTO login_attempts (at, ok) VALUES (?1, 1)", [db::now()])?;
    conn.execute("DELETE FROM login_attempts WHERE at < ?1", [&since])?;
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let token = hex::encode(raw);
    let now = db::now();
    conn.execute(
        "INSERT INTO devices (id, name, token_hash, created_at, last_seen) VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params![db::new_id(), device.chars().take(60).collect::<String>(), hash_token(&token), now],
    )?;
    Ok(token)
}

/// The signed-in device, from `Authorization: Bearer <token>`.
pub struct Device {
    pub id: String,
}

#[axum::async_trait]
impl FromRequestParts<AppState> for Device {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthorized)?
            .trim()
            .to_string();
        let pool = state.pool.clone();
        tokio::task::spawn_blocking(move || -> ApiResult<Device> {
            let conn = pool.get()?;
            let id: String = conn
                .query_row("SELECT id FROM devices WHERE token_hash = ?1", [hash_token(&token)], |r| r.get(0))
                .map_err(|_| ApiError::unauthorized())?;
            conn.execute("UPDATE devices SET last_seen = ?1 WHERE id = ?2", [db::now(), id.clone()])?;
            Ok(Device { id })
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_needs_password_and_a_fresh_code() {
        let pool = db::memory();
        let c = pool.get().unwrap();
        let e = create_user(&c, "Daan@example.com", "een lang wachtwoord").unwrap();
        assert_eq!(e.recovery_codes.len(), 10);
        let sealed: String = c.query_row("SELECT totp_sealed FROM users", [], |r| r.get(0)).unwrap();
        let secret = totp::open(&sealed).unwrap();
        let code = format!("{:06}", totp::code_at(&secret, chrono::Utc::now().timestamp() / totp::STEP));
        assert!(login(&c, "daan@example.com", "fout wachtwoord!", &code, "test").is_err());
        let token = login(&c, "daan@example.com", "een lang wachtwoord", &code, "telefoon").unwrap();
        assert_eq!(token.len(), 64);
        assert!(login(&c, "daan@example.com", "een lang wachtwoord", &code, "x").is_err(), "a code works once");
        assert!(login(&c, "daan@example.com", "een lang wachtwoord", &e.recovery_codes[0], "x").is_ok());
        assert!(login(&c, "daan@example.com", "een lang wachtwoord", &e.recovery_codes[0], "x").is_err());
    }
}
