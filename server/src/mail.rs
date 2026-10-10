//! E-mail through Brevo's HTTP API, the same provider and key setup as
//! Krentenweger (`BREVO_API_KEY`, `EMAIL_FROM_ADDRESS`, `EMAIL_FROM_NAME`).

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::json;

pub struct Attachment {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn configured() -> bool {
    crate::config::brevo_key().is_some() && std::env::var("EMAIL_FROM_ADDRESS").map(|v| !v.is_empty()).unwrap_or(false)
}

pub async fn send(to: &[&str], subject: &str, html: &str, attachment: Option<Attachment>) -> Result<()> {
    let key = crate::config::brevo_key().context("BREVO_API_KEY ontbreekt")?;
    let from = std::env::var("EMAIL_FROM_ADDRESS").context("EMAIL_FROM_ADDRESS ontbreekt")?;
    let name = std::env::var("EMAIL_FROM_NAME").unwrap_or_else(|_| "Boekhouding".into());
    let mut body = json!({
        "sender": {"email": from, "name": name},
        "to": to.iter().filter(|t| !t.is_empty()).map(|t| json!({"email": t})).collect::<Vec<_>>(),
        "subject": subject,
        "htmlContent": html,
    });
    if let Some(a) = attachment {
        body["attachment"] = json!([{ "name": a.name, "content": base64::engine::general_purpose::STANDARD.encode(a.bytes) }]);
    }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build()?;
    let res = client.post("https://api.brevo.com/v3/smtp/email").header("api-key", key).json(&body).send().await?;
    if !res.status().is_success() {
        bail!("Brevo weigerde de mail: {} {}", res.status(), res.text().await.unwrap_or_default());
    }
    Ok(())
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
