//! Time-based one-time passwords (RFC 6238) for the one account.
//!
//! Copied from krentenweger-web's `totp.rs`. Codes are HMAC-SHA1, six digits,
//! 30-second steps, one step either side of now; a used step is refused
//! afterwards (`users.totp_last_step`). The secret is stored sealed with
//! ChaCha20-Poly1305 under a key derived from `BOEK_SECRET`. Ten recovery
//! codes, shown once, stored as SHA-256 hashes.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Seconds per code.
pub const STEP: i64 = 30;
/// Steps either side of now that still count, for a phone clock a little off.
pub const WINDOW: i64 = 1;

/// The six-digit code for one time step (RFC 4226 truncation).
pub fn code_at(secret: &[u8], step: i64) -> u32 {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret).expect("hmac takes any key length");
    mac.update(&(step as u64).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let bin = u32::from_be_bytes([digest[offset] & 0x7f, digest[offset + 1], digest[offset + 2], digest[offset + 3]]);
    bin % 1_000_000
}

/// The time step a typed code matches within the window around `now_unix`.
/// The caller refuses a step at or before the last one used.
pub fn matching_step(secret: &[u8], code: &str, now_unix: i64) -> Option<i64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let wanted: u32 = code.parse().ok()?;
    let now = now_unix.div_euclid(STEP);
    (now - WINDOW..=now + WINDOW).find(|&step| code_at(secret, step) == wanted)
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 without padding, as authenticator apps take a secret.
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(B32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// A fresh 160-bit secret.
pub fn new_secret() -> Vec<u8> {
    let mut s = vec![0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut s);
    s
}

/// The `otpauth://` link an authenticator app reads, from the QR or a tap.
pub fn otpauth_uri(secret: &[u8], account: &str) -> String {
    format!(
        "otpauth://totp/Boekhouding:{}?secret={}&issuer=Boekhouding&algorithm=SHA1&digits=6&period=30",
        urlencoding::encode(account),
        base32(secret)
    )
}

/// The QR code of `text` as rows of `1` (dark) and `0`, without the quiet
/// zone; the app and the web page draw it.
pub fn qr_rows(text: &str) -> Vec<String> {
    let Ok(code) = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M) else {
        return Vec::new();
    };
    let width = code.width();
    code.to_colors()
        .chunks(width)
        .map(|row| row.iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect())
        .collect()
}

/// Ten codes like `k7m2-x9qp`, from an alphabet without look-alikes.
pub fn new_recovery_codes() -> Vec<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::rngs::OsRng;
    (0..10)
        .map(|_| {
            let mut c = String::with_capacity(9);
            for i in 0..8 {
                if i == 4 {
                    c.push('-');
                }
                c.push(ALPHABET[(rng.next_u32() as usize) % ALPHABET.len()] as char);
            }
            c
        })
        .collect()
}

/// What is stored for a recovery code: a SHA-256 of it after the clean-up a
/// typed code gets (case, spaces and the dash do not matter). The codes are
/// random and attempts are rate-limited, so a slow hash adds nothing.
pub fn hash_recovery(code: &str) -> String {
    let clean: String = code.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    hex::encode(Sha256::digest(format!("boekhouding-recovery:{clean}")))
}

fn cipher() -> ChaCha20Poly1305 {
    let master = crate::config::secret();
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&master).expect("hmac takes any key length");
    mac.update(b"boekhouding-totp-secret-key-v1");
    let key = mac.finalize().into_bytes();
    ChaCha20Poly1305::new(Key::from_slice(&key))
}

/// The secret sealed for storage: `hex(nonce ‖ ciphertext+tag)`.
pub fn seal(secret: &[u8]) -> String {
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let sealed = cipher().encrypt(Nonce::from_slice(&nonce), secret).expect("sealing a short secret cannot fail");
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    hex::encode(out)
}

/// The secret, or `None` when it was sealed under another key or altered.
pub fn open(stored: &str) -> Option<Vec<u8>> {
    let raw = hex::decode(stored).ok()?;
    if raw.len() <= 12 {
        return None;
    }
    let (nonce, sealed) = raw.split_at(12);
    cipher().decrypt(Nonce::from_slice(nonce), sealed).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, the SHA-1 rows, last six digits.
    #[test]
    fn codes_match_rfc_6238() {
        let secret = b"12345678901234567890";
        for (t, code) in [(59, 287082), (1111111109, 81804), (1111111111, 50471), (1234567890, 5924), (2000000000, 279037), (20000000000, 353130)] {
            assert_eq!(code_at(secret, t / STEP), code, "at {t}");
        }
        assert_eq!(matching_step(secret, "287082", 59), Some(1));
        assert_eq!(matching_step(secret, "287 082", 59 + 30), Some(1), "one step late still counts");
        assert_eq!(matching_step(secret, "287082", 59 + 90), None, "three steps late does not");
        assert_eq!(matching_step(secret, "28708", 59), None);
        assert_eq!(matching_step(secret, "abcdef", 59), None);
    }

    #[test]
    fn base32_link_and_qr() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(b"12345678901234567890"), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        let uri = otpauth_uri(b"12345678901234567890", "a@b.nl");
        assert!(uri.starts_with("otpauth://totp/Boekhouding:a%40b.nl?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&"));
        let rows = qr_rows(&uri);
        assert!(rows.len() >= 21 && rows.iter().all(|r| r.len() == rows.len()));
        // A finder pattern in the top-left corner.
        assert!(rows[0].starts_with("1111111"));
    }

    #[test]
    fn a_sealed_secret_opens_and_an_altered_one_does_not() {
        let secret = new_secret();
        let sealed = seal(&secret);
        assert_eq!(open(&sealed).as_deref(), Some(secret.as_slice()));
        assert_ne!(seal(&secret), sealed, "a fresh nonce each time");
        assert!(!sealed.contains(&hex::encode(&secret)));
        let mut bad = hex::decode(&sealed).unwrap();
        bad[14] ^= 1;
        assert_eq!(open(&hex::encode(bad)), None);
    }

    #[test]
    fn recovery_codes_are_ten_and_hash_the_same_however_typed() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|c| c.len() == 9 && c.as_bytes()[4] == b'-'));
        assert_eq!(hash_recovery(&codes[0]), hash_recovery(&format!(" {} ", codes[0].to_uppercase().replace('-', ""))));
        assert_ne!(hash_recovery(&codes[0]), hash_recovery(&codes[1]));
    }
}
