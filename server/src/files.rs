use crate::config;
use crate::error::{ApiError, ApiResult};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const MAX_BYTES: usize = 25 * 1024 * 1024;

fn dir() -> PathBuf {
    config::data_dir().join("files")
}

pub fn path(sha: &str) -> ApiResult<PathBuf> {
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad("Ongeldige bestandsnaam"));
    }
    Ok(dir().join(sha.to_ascii_lowercase()))
}

pub fn store(bytes: &[u8]) -> ApiResult<String> {
    if bytes.is_empty() {
        return Err(ApiError::bad("Leeg bestand"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(ApiError::bad("Bestand is groter dan 25 MB"));
    }
    let sha = hex::encode(Sha256::digest(bytes));
    std::fs::create_dir_all(dir())?;
    let p = path(&sha)?;
    if !p.exists() {
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &p)?;
    }
    Ok(sha)
}

pub fn sniff(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"%PDF") {
        "application/pdf"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<Document") {
        "application/xml"
    } else {
        "application/octet-stream"
    }
}

pub async fn pdf_text(bytes: &[u8]) -> ApiResult<String> {
    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::write(tmp.path(), bytes)?;
    let out = tokio::process::Command::new("pdftotext")
        .arg("-layout")
        .arg("-enc")
        .arg("UTF-8")
        .arg(tmp.path())
        .arg("-")
        .output()
        .await
        .map_err(|e| ApiError(axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("pdftotext ontbreekt op de server: {e}")))?;
    if !out.status.success() {
        return Err(ApiError::bad("Deze PDF kan niet gelezen worden"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
