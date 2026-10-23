//! Time-based one-time codes (RFC 6238: HMAC-SHA1, 6 digits, 30 seconds) and recovery codes
//! (16 Crockford base32 characters, 80 bits), as decision 0012 fixes them.

use hmac::{Hmac, KeyInit, Mac};
use holocron_protocol::limits::{RECOVERY_CODES, TOTP_DIGITS, TOTP_PERIOD_SECONDS};
use sha1::Sha1;

use super::token_digest;

/// 160-bit shared secrets.
pub const SECRET_BYTES: usize = 20;
const RFC4648: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const RECOVERY_CODE_BYTES: usize = 10;

pub fn new_secret() -> [u8; SECRET_BYTES] {
    let mut secret = [0u8; SECRET_BYTES];
    getrandom::fill(&mut secret).expect("the OS provides randomness");
    secret
}

/// Base32 without padding, as authenticator apps expect for manual entry.
pub fn base32(bytes: &[u8]) -> String {
    encode(bytes, RFC4648)
}

/// The time step a Unix millisecond instant falls in.
pub fn step(now_ms: i64) -> i64 {
    now_ms.div_euclid(1000 * i64::from(TOTP_PERIOD_SECONDS))
}

/// The code of one time step.
pub fn code(secret: &[u8], step: i64) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("HMAC takes keys of any size");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[hash.len() - 1] & 0x0f);
    let binary = u32::from_be_bytes([
        hash[offset] & 0x7f,
        hash[offset + 1],
        hash[offset + 2],
        hash[offset + 3],
    ]);
    format!(
        "{:0width$}",
        binary % 10u32.pow(TOTP_DIGITS),
        width = TOTP_DIGITS as usize
    )
}

/// The step `input` is valid for, one step of clock skew either way, never one at or before
/// `last_step`, so each step is accepted once.
pub fn verify(secret: &[u8], input: &str, now_ms: i64, last_step: Option<i64>) -> Option<i64> {
    let input: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if input.len() != TOTP_DIGITS as usize || !input.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let current = step(now_ms);
    (current - 1..=current + 1)
        .filter(|candidate| last_step.is_none_or(|last| *candidate > last))
        .find(|candidate| constant_time_eq(code(secret, *candidate).as_bytes(), input.as_bytes()))
}

/// 10 new recovery codes, shown as `XXXX-XXXX-XXXX-XXXX`, with the digests to store.
pub fn new_recovery_codes() -> Vec<(String, String)> {
    (0..RECOVERY_CODES)
        .map(|_| {
            let mut bytes = [0u8; RECOVERY_CODE_BYTES];
            getrandom::fill(&mut bytes).expect("the OS provides randomness");
            let raw = encode(&bytes, CROCKFORD);
            let shown = raw
                .as_bytes()
                .chunks(4)
                .map(|chunk| std::str::from_utf8(chunk).expect("ASCII"))
                .collect::<Vec<_>>()
                .join("-");
            let digest = token_digest(&raw);
            (shown, digest)
        })
        .collect()
}

/// The stored digest of a recovery code as typed: spaces and dashes dropped, case ignored, and
/// the letters Crockford base32 reads as digits (`O`, `I`, `L`) mapped to them.
pub fn recovery_code_digest(input: &str) -> String {
    let normalized: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect();
    token_digest(&normalized)
}

fn encode(bytes: &[u8], alphabet: &[u8; 32]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(alphabet[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(alphabet[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA1 rows, truncated to 6 digits.
    #[test]
    fn codes_match_the_rfc_test_vectors() {
        let secret = b"12345678901234567890";
        for (seconds, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code(secret, step(seconds * 1000)), expected);
        }
        assert_eq!(base32(secret), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    }

    #[test]
    fn a_code_is_accepted_within_one_step_and_only_once() {
        let secret = new_secret();
        let now = 1_790_848_800_000;
        let next = code(&secret, step(now) + 1);
        assert_eq!(verify(&secret, &next, now, None), Some(step(now) + 1));
        assert_eq!(verify(&secret, &next, now, Some(step(now) + 1)), None);
        assert_eq!(
            verify(&secret, &code(&secret, step(now) + 2), now, None),
            None
        );
        assert_eq!(verify(&secret, "12345", now, None), None);
    }

    #[test]
    fn recovery_codes_compare_without_dashes_or_case() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), RECOVERY_CODES);
        let (shown, digest) = &codes[0];
        assert_eq!(shown.len(), 19);
        assert_eq!(&recovery_code_digest(shown), digest);
        assert_eq!(
            &recovery_code_digest(&shown.replace('-', " ").to_lowercase()),
            digest
        );
    }
}
