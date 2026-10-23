//! The `x-umami-cache` token: a signed claim of the website, session and visit an earlier
//! answer named (0005 Cache tokens bound to their website). Shaped like a JWT so clients that
//! inspect it still see three dot-separated parts, signed with BLAKE3 under a key derived
//! from the root secret. Upstream tokens fail the check and are ignored.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const HEADER: &str = r#"{"alg":"BLAKE3","typ":"JWT"}"#;
const KIND: &str = "cache";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    /// Absent for link and pixel sources, as upstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_id: Option<Uuid>,
    pub session_id: Uuid,
    pub visit_id: Uuid,
    /// Start of the visit, Unix seconds.
    pub iat: i64,
    #[serde(rename = "type")]
    pub kind: String,
}

impl Claims {
    pub fn new(website_id: Option<Uuid>, session_id: Uuid, visit_id: Uuid, iat: i64) -> Self {
        Self {
            website_id,
            session_id,
            visit_id,
            iat,
            kind: KIND.into(),
        }
    }
}

pub struct TokenKey([u8; 32]);

impl TokenKey {
    pub fn derive(root: &[u8; 32]) -> Self {
        Self(blake3::derive_key("holocron/v1/cache-token", root))
    }

    pub fn sign(&self, claims: &Claims) -> String {
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(HEADER),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims serialize"))
        );
        let mac = blake3::keyed_hash(&self.0, signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(mac.as_bytes()))
    }

    /// The claims of a token this installation signed, else `None`.
    pub fn verify(&self, token: &str) -> Option<Claims> {
        let (signed, mac) = token.rsplit_once('.')?;
        let mac: [u8; 32] = URL_SAFE_NO_PAD.decode(mac).ok()?.try_into().ok()?;
        // Hash equality is constant time.
        if blake3::keyed_hash(&self.0, signed.as_bytes()) != blake3::Hash::from_bytes(mac) {
            return None;
        }
        let (_, claims) = signed.split_once('.')?;
        let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).ok()?).ok()?;
        (claims.kind == KIND).then_some(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_verify_only_under_their_key() {
        let key = TokenKey::derive(&[7; 32]);
        let claims = Claims::new(
            Some(Uuid::from_u128(1)),
            Uuid::from_u128(2),
            Uuid::from_u128(3),
            1_790_000_000,
        );
        let token = key.sign(&claims);
        assert_eq!(token.split('.').count(), 3);
        assert_eq!(key.verify(&token), Some(claims));
        assert_eq!(TokenKey::derive(&[8; 32]).verify(&token), None);
        assert_eq!(key.verify(&token.replace('.', "x.")), None);
    }
}
